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

use sl_types::lsl::{Rotation, Vector};
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
    ObjectPropertiesFamily as ObjectPropertiesFamilyMessage,
    ObjectPropertiesFamilyObjectDataBlock as ObjectPropertiesFamilyObjectDataBlockMessage,
    ParcelInfoReply, ParcelInfoReplyAgentDataBlock, ParcelInfoReplyDataBlock,
    ParcelObjectOwnersReply, ParcelObjectOwnersReplyDataBlock, PayPriceReply,
    PayPriceReplyButtonDataBlock, PayPriceReplyObjectDataBlock, TelehubInfo as TelehubInfoMessage,
    TelehubInfoSpawnPointBlockBlock, TelehubInfoTelehubBlockBlock,
};
use sl_wire::{
    AnyMessage, ControlFlags, EventQueueEvent, Llsd, MessageId, PacketFlags, Reader, WireError,
    Writer, build_event_queue_response, encode_datagram, parse_datagram, zero_decode,
};
use uuid::Uuid;

use crate::error::Error;
use crate::session::{
    build_map_block_reply, build_map_item_reply, instant_message, region_handshake_message,
};
use crate::types::directory::category_from_wire;
use crate::types::{
    AttachmentPoint, AvatarName, AvatarPickerResult, Camera, ChatType, CoarseLocation,
    DirClassifiedResult, DirEventResult, DirFindFlags, DirGroupResult, DirLandResult,
    DirPeopleResult, DirPlaceResult, EstateCovenant, EventInfo, GroupName, InstantMessage,
    LandSearchType, MapItem, MapItemType, MapRegionInfo, NotecardRez, ObjectBuyItem,
    ObjectPropertiesFamily, ParcelCategory, ParcelDetails, ParcelObjectOwner, PlacesResult,
    RegionIdentity, Reliability, RestoreItem, RezAttachment, SaleType, TelehubInfo, Throttle,
    Transmit, ViewerEffect, ViewerEffectData, ViewerEffectType,
};

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
        agent_id: Uuid,
        /// The session id.
        session_id: Uuid,
        /// The circuit code.
        circuit_code: u32,
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
        ping_id: u8,
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
        channel: i32,
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
        local_id: u32,
        /// The point the object is attached to.
        attachment_point: AttachmentPoint,
        /// Whether the object was added to the point (`true`) rather than
        /// replacing what was there.
        add: bool,
        /// The rotation the object is worn at.
        rotation: Rotation,
    },
    /// The client detached attachments back to inventory (`ObjectDetach`).
    DetachObjects(Vec<u32>),
    /// The client dropped attachments onto the ground (`ObjectDrop`).
    DropAttachments(Vec<u32>),
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
        first_detach_all: bool,
        /// The items the client wore.
        attachments: Vec<RezAttachment>,
    },
    /// The client emitted one or more viewer effects (`ViewerEffect`): look-at /
    /// point-at gaze hints, the editing/touch beam, and other transient HUD
    /// effects. A simulator would relay these to other nearby viewers.
    ViewerEffect(Vec<ViewerEffect>),
    /// The client asked to track an agent's position (`TrackAgent`); the
    /// simulator would stream the tracked agent's coarse location back via
    /// [`SimSession::send_coarse_location_update`].
    TrackAgent {
        /// The agent to track.
        prey_id: Uuid,
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
        /// The classified category to filter by (`0` for any).
        category: u32,
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
        event_id: u32,
    },
    /// The client subscribed to a reminder for an in-world event
    /// (`EventNotificationAddRequest`). There is no direct reply.
    EventNotificationAddRequest {
        /// The event to be reminded about.
        event_id: u32,
    },
    /// The client cancelled an event reminder (`EventNotificationRemoveRequest`).
    /// There is no direct reply.
    EventNotificationRemoveRequest {
        /// The event whose reminder to cancel.
        event_id: u32,
    },
    /// The client wants to buy in-world objects (`ObjectBuy`).
    BuyObject {
        /// The active group ([`Uuid::nil`] for none).
        group_id: Uuid,
        /// The inventory folder a derezed purchase is placed in.
        category_id: Uuid,
        /// The objects to buy (each with its advertised sale type and price).
        objects: Vec<ObjectBuyItem>,
    },
    /// The client wants to buy an item out of an object's contents
    /// (`BuyObjectInventory`).
    BuyObjectInventory {
        /// The object whose contents holds the item.
        object_id: Uuid,
        /// The inventory item to buy.
        item_id: Uuid,
        /// The folder the bought item is placed in.
        folder_id: Uuid,
    },
    /// The client asked for an object's pay-button layout (`RequestPayPrice`);
    /// the simulator answers with [`SimSession::send_pay_price_reply`].
    RequestPayPrice {
        /// The object queried.
        object_id: Uuid,
    },
    /// The client asked for an object's condensed broadcast properties
    /// (`RequestObjectPropertiesFamily`); the simulator answers with
    /// [`SimSession::send_object_properties_family`].
    RequestObjectPropertiesFamily {
        /// The request flags, echoed back in the reply.
        request_flags: u32,
        /// The object queried.
        object_id: Uuid,
    },
    /// The client began an interactive object spin (`ObjectSpinStart`).
    SpinObjectStart {
        /// The object being spun.
        object_id: Uuid,
    },
    /// The client updated an in-progress object spin (`ObjectSpinUpdate`).
    SpinObjectUpdate {
        /// The object being spun.
        object_id: Uuid,
        /// The new rotation.
        rotation: Rotation,
    },
    /// The client ended an interactive object spin (`ObjectSpinStop`).
    SpinObjectStop {
        /// The object being spun.
        object_id: Uuid,
    },
    /// The client wants to duplicate objects onto a raycast surface
    /// (`ObjectDuplicateOnRay`).
    DuplicateObjectsOnRay {
        /// The region-local ids to duplicate.
        local_ids: Vec<u32>,
        /// The active group the copies are set to ([`Uuid::nil`] for none).
        group_id: Uuid,
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
        /// The object the ray is cast against ([`Uuid::nil`] for the terrain).
        ray_target_id: Uuid,
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
        local_id: i32,
    },
    /// The client wants to buy a temporary access pass to a parcel
    /// (`ParcelBuyPass`).
    BuyParcelPass {
        /// The parcel's region-local id.
        local_id: i32,
    },
    /// The client wants to disable scripted objects on a parcel
    /// (`ParcelDisableObjects`).
    DisableParcelObjects {
        /// The parcel's region-local id.
        local_id: i32,
        /// Which objects to disable (combined `ParcelReturnType` constants).
        return_type: u32,
        /// The owner-id scope (empty for none).
        owner_ids: Vec<Uuid>,
        /// The explicit object/task-id scope (empty for none).
        task_ids: Vec<Uuid>,
    },
    /// The client asked for a parcel's basic listing by grid-wide parcel id
    /// (`ParcelInfoRequest`); the simulator answers with
    /// [`SimSession::send_parcel_info_reply`].
    RequestParcelInfo {
        /// The parcel's grid-wide id.
        parcel_id: Uuid,
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
        object_local_id: u32,
    },
    /// The client asked to remove the region's telehub (`EstateOwnerMessage`/
    /// `telehub` `delete`).
    DisconnectTelehub,
    /// The client asked to add a telehub spawn point at an object's position
    /// (`EstateOwnerMessage`/`telehub` `spawnpoint add`).
    AddTelehubSpawnPoint {
        /// The local id of the object marking the spawn point.
        object_local_id: u32,
    },
    /// The client asked to remove a telehub spawn point by index
    /// (`EstateOwnerMessage`/`telehub` `spawnpoint remove`).
    RemoveTelehubSpawnPoint {
        /// The zero-based index of the spawn point to remove.
        spawn_index: u32,
    },
    /// The client requested a clean logout (`LogoutRequest`); the simulator has
    /// replied with a `LogoutReply` and closed the session.
    LoggedOut,
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
    region_handle: u64,
    /// The channel/version string reported in `AgentMovementComplete`.
    channel_version: Vec<u8>,
    /// The client's UDP address, learned from the first inbound datagram.
    client_addr: Option<SocketAddr>,
    /// The agent id, from `UseCircuitCode`.
    agent_id: Option<Uuid>,
    /// The session id, from `UseCircuitCode`.
    session_id: Option<Uuid>,
    /// The circuit code, from `UseCircuitCode`.
    circuit_code: Option<u32>,
    /// The next outgoing sequence number.
    next_sequence: u32,
    /// The next `StartPingCheck` ping id.
    next_ping_id: u8,
    /// Inbound reliable sequence numbers we still owe acknowledgements for.
    pending_acks: Vec<u32>,
    /// Outgoing reliable packets awaiting acknowledgement, keyed by sequence.
    unacked: BTreeMap<u32, UnackedPacket>,
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
    pub fn new(region_handle: u64, now: Instant) -> Self {
        Self {
            state: SimState::AwaitingCircuit,
            region_handle,
            channel_version: b"sl-proto SimSession".to_vec(),
            client_addr: None,
            agent_id: None,
            session_id: None,
            circuit_code: None,
            next_sequence: 1,
            next_ping_id: 1,
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
    pub const fn agent_id(&self) -> Option<Uuid> {
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
    const fn next_sequence(&mut self) -> u32 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
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
        source_id: Uuid,
        owner_id: Uuid,
        source_type: u8,
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
                source_id,
                owner_id,
                source_type,
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
        flags: u32,
        regions: &[MapRegionInfo],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let agent_id = self.agent_id.unwrap_or_else(Uuid::nil);
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
        flags: u32,
        item_type: MapItemType,
        items: &[MapItem],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let agent_id = self.agent_id.unwrap_or_else(Uuid::nil);
        let message =
            AnyMessage::MapItemReply(build_map_item_reply(agent_id, flags, item_type, items));
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
                        id: name.id,
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
                        id: name.id,
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
                    agent_id: location.agent_id,
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
        source_agent: Uuid,
        effects: &[ViewerEffect],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::ViewerEffect(ViewerEffectMessage {
            agent_data: ViewerEffectAgentDataBlock {
                agent_id: source_agent,
                session_id: Uuid::nil(),
            },
            effect: effects
                .iter()
                .map(|effect| ViewerEffectEffectBlock {
                    id: effect.id,
                    agent_id: effect.agent_id,
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
                agent_id: self.agent_id.unwrap_or_else(Uuid::nil),
            },
            query_data: DirPeopleReplyQueryDataBlock { query_id },
            query_replies: results
                .iter()
                .map(|result| DirPeopleReplyQueryRepliesBlock {
                    agent_id: result.agent_id,
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
                agent_id: self.agent_id.unwrap_or_else(Uuid::nil),
            },
            query_data: DirGroupsReplyQueryDataBlock { query_id },
            query_replies: results
                .iter()
                .map(|result| DirGroupsReplyQueryRepliesBlock {
                    group_id: result.group_id,
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
                agent_id: self.agent_id.unwrap_or_else(Uuid::nil),
            },
            query_data: DirEventsReplyQueryDataBlock { query_id },
            query_replies: results
                .iter()
                .map(|result| DirEventsReplyQueryRepliesBlock {
                    owner_id: result.owner_id,
                    name: with_nul(&result.name),
                    event_id: result.event_id,
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
                agent_id: self.agent_id.unwrap_or_else(Uuid::nil),
            },
            query_data: DirClassifiedReplyQueryDataBlock { query_id },
            query_replies: results
                .iter()
                .map(|result| DirClassifiedReplyQueryRepliesBlock {
                    classified_id: result.classified_id,
                    name: with_nul(&result.name),
                    classified_flags: result.classified_flags,
                    creation_date: result.creation_date,
                    expiration_date: result.expiration_date,
                    price_for_listing: result.price_for_listing,
                })
                .collect(),
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
                agent_id: self.agent_id.unwrap_or_else(Uuid::nil),
            },
            query_data: vec![DirPlacesReplyQueryDataBlock { query_id }],
            query_replies: results
                .iter()
                .map(|result| DirPlacesReplyQueryRepliesBlock {
                    parcel_id: result.parcel_id,
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
                agent_id: self.agent_id.unwrap_or_else(Uuid::nil),
            },
            query_data: DirLandReplyQueryDataBlock { query_id },
            query_replies: results
                .iter()
                .map(|result| DirLandReplyQueryRepliesBlock {
                    parcel_id: result.parcel_id,
                    name: with_nul(&result.name),
                    auction: result.auction,
                    for_sale: result.for_sale,
                    sale_price: result.sale_price,
                    actual_area: result.actual_area,
                })
                .collect(),
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
                agent_id: self.agent_id.unwrap_or_else(Uuid::nil),
                query_id,
            },
            data: results
                .iter()
                .map(|result| AvatarPickerReplyDataBlock {
                    avatar_id: result.avatar_id,
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
                agent_id: self.agent_id.unwrap_or_else(Uuid::nil),
                query_id,
            },
            transaction_data: PlacesReplyTransactionDataBlock { transaction_id },
            query_data: results
                .iter()
                .map(|result| PlacesReplyQueryDataBlock {
                    owner_id: result.owner_id,
                    name: with_nul(&result.name),
                    desc: with_nul(&result.description),
                    actual_area: result.actual_area,
                    billable_area: result.billable_area,
                    flags: result.flags,
                    global_x: result.global_position.0,
                    global_y: result.global_position.1,
                    global_z: result.global_position.2,
                    sim_name: with_nul(&result.sim_name),
                    snapshot_id: result.snapshot_id,
                    dwell: result.dwell,
                    price: result.price,
                })
                .collect(),
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
        let (global_x, global_y, global_z) = info.global_position;
        let message = AnyMessage::EventInfoReply(EventInfoReply {
            agent_data: EventInfoReplyAgentDataBlock {
                agent_id: self.agent_id.unwrap_or_else(Uuid::nil),
            },
            event_data: EventInfoReplyEventDataBlock {
                event_id: info.event_id,
                creator: with_nul(&info.creator.to_string()),
                name: with_nul(&info.name),
                category: with_nul(&info.category),
                desc: with_nul(&info.description),
                date: with_nul(&info.date),
                date_utc: info.date_utc,
                duration: info.duration,
                cover: info.cover,
                amount: info.amount,
                sim_name: with_nul(&info.sim_name),
                global_pos: [global_x, global_y, global_z],
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
        object_id: Uuid,
        default_pay_price: i32,
        pay_buttons: &[i32],
        now: Instant,
    ) -> Result<(), Error> {
        if self.client_addr.is_none() {
            return Err(Error::NoCircuit);
        }
        let message = AnyMessage::PayPriceReply(PayPriceReply {
            object_data: PayPriceReplyObjectDataBlock {
                object_id,
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
        let message = AnyMessage::ObjectPropertiesFamily(ObjectPropertiesFamilyMessage {
            object_data: ObjectPropertiesFamilyObjectDataBlockMessage {
                request_flags: properties.request_flags,
                object_id: properties.object_id,
                owner_id: properties.owner_id,
                group_id: properties.group_id,
                base_mask: properties.base_mask,
                owner_mask: properties.owner_mask,
                group_mask: properties.group_mask,
                everyone_mask: properties.everyone_mask,
                next_owner_mask: properties.next_owner_mask,
                ownership_cost: properties.ownership_cost,
                sale_type: properties.sale_type,
                sale_price: properties.sale_price,
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
                    owner_id: owner.owner_id,
                    is_group_owned: owner.is_group_owned,
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
                agent_id: self.agent_id.unwrap_or_else(Uuid::nil),
            },
            data: ParcelInfoReplyDataBlock {
                parcel_id: details.parcel_id,
                owner_id: details.owner_id,
                name: with_nul(&details.name),
                desc: with_nul(&details.description),
                actual_area: details.actual_area,
                billable_area: details.billable_area,
                flags: details.flags,
                global_x: details.global_x,
                global_y: details.global_y,
                global_z: details.global_z,
                sim_name: with_nul(&details.sim_name),
                snapshot_id: details.snapshot_id,
                dwell: details.dwell,
                sale_price: details.sale_price,
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
                covenant_id: covenant.covenant_id,
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
                object_id: info.object_id,
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

    /// Sends a `StartPingCheck` to the client; the client answers with a
    /// `CompletePingCheck`. Returns the ping id sent (so a caller can match the
    /// reply), or `None` if the circuit is not open.
    ///
    /// # Errors
    ///
    /// Returns a wire error if the message fails to encode.
    pub fn start_ping_check(&mut self, now: Instant) -> Result<Option<u8>, Error> {
        if self.client_addr.is_none() {
            return Ok(None);
        }
        let ping_id = self.next_ping_id;
        self.next_ping_id = self.next_ping_id.wrapping_add(1);
        let oldest_unacked = self.unacked.keys().next().copied().unwrap_or(0);
        let message = AnyMessage::StartPingCheck(StartPingCheck {
            ping_id: StartPingCheckPingIDBlock {
                ping_id,
                oldest_unacked,
            },
        });
        self.send(&message, Reliability::Unreliable, now)?;
        Ok(Some(ping_id))
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
    fn queue_ack(&mut self, sequence: u32, now: Instant) {
        self.pending_acks.push(sequence);
        if self.ack_flush.is_none() {
            self.ack_flush = Some(deadline(now, ACK_FLUSH_DELAY));
        }
    }

    /// Removes the given outgoing sequence numbers from the unacked set.
    fn record_acks(&mut self, ids: &[u32]) {
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
                .map(|id| sl_wire::messages::PacketAckPacketsBlock { id: *id })
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
                self.agent_id = Some(block.id);
                self.session_id = Some(block.session_id);
                self.circuit_code = Some(block.code);
                if matches!(self.state, SimState::AwaitingCircuit) {
                    self.state = SimState::Active;
                    self.ping = Some(deadline(now, PING_INTERVAL));
                }
                self.events.push_back(ServerEvent::CircuitOpened {
                    agent_id: block.id,
                    session_id: block.session_id,
                    circuit_code: block.code,
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
                let ping_id = ping.ping_id.ping_id;
                let reply = AnyMessage::CompletePingCheck(CompletePingCheck {
                    ping_id: CompletePingCheckPingIDBlock { ping_id },
                });
                self.send(&reply, Reliability::Unreliable, now)?;
                self.events
                    .push_back(ServerEvent::PingRequested { ping_id });
            }
            // The client answering our periodic `StartPingCheck`; consumed.
            AnyMessage::CompletePingCheck(_) => {}
            AnyMessage::PacketAck(ack) => {
                let ids: Vec<u32> = ack.packets.iter().map(|packet| packet.id).collect();
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
                        camera: Camera::new(
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
                    channel: chat.chat_data.channel,
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
                let (attachment_point, add) =
                    AttachmentPoint::split_code(attach.agent_data.attachment_point);
                for object in &attach.object_data {
                    self.events.push_back(ServerEvent::AttachObject {
                        local_id: object.object_local_id,
                        attachment_point,
                        add,
                        rotation: object.rotation.clone(),
                    });
                }
            }
            AnyMessage::ObjectDetach(detach) => {
                let ids = detach
                    .object_data
                    .iter()
                    .map(|object| object.object_local_id)
                    .collect();
                self.events.push_back(ServerEvent::DetachObjects(ids));
            }
            AnyMessage::ObjectDrop(drop) => {
                let ids = drop
                    .object_data
                    .iter()
                    .map(|object| object.object_local_id)
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
                let (attachment_point, add) = AttachmentPoint::split_code(object.attachment_pt);
                self.events
                    .push_back(ServerEvent::RezAttachment(Box::new(RezAttachment {
                        item_id: object.item_id,
                        owner_id: object.owner_id,
                        attachment_point,
                        add,
                        name: trimmed_string(&object.name),
                        description: trimmed_string(&object.description),
                    })));
            }
            AnyMessage::RezMultipleAttachmentsFromInv(rez) => {
                let attachments = rez
                    .object_data
                    .iter()
                    .map(|object| {
                        let (attachment_point, add) =
                            AttachmentPoint::split_code(object.attachment_pt);
                        RezAttachment {
                            item_id: object.item_id,
                            owner_id: object.owner_id,
                            attachment_point,
                            add,
                            name: trimmed_string(&object.name),
                            description: trimmed_string(&object.description),
                        }
                    })
                    .collect();
                self.events.push_back(ServerEvent::RezAttachments {
                    compound_id: rez.header_data.compound_msg_id,
                    first_detach_all: rez.header_data.first_detach_all,
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
                            agent_id: block.agent_id,
                            effect_type,
                            duration: block.duration,
                            color: block.color,
                            data: ViewerEffectData::from_wire(effect_type, &block.type_data),
                        }
                    })
                    .collect();
                self.events.push_back(ServerEvent::ViewerEffect(effects));
            }
            AnyMessage::TrackAgent(track) => {
                self.events.push_back(ServerEvent::TrackAgent {
                    prey_id: track.target_data.prey_id,
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
                    category: query.query_data.category,
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
                    event_id: request.event_data.event_id,
                });
            }
            AnyMessage::EventNotificationAddRequest(request) => {
                self.events
                    .push_back(ServerEvent::EventNotificationAddRequest {
                        event_id: request.event_data.event_id,
                    });
            }
            AnyMessage::EventNotificationRemoveRequest(request) => {
                self.events
                    .push_back(ServerEvent::EventNotificationRemoveRequest {
                        event_id: request.event_data.event_id,
                    });
            }
            AnyMessage::ObjectBuy(buy) => {
                self.events.push_back(ServerEvent::BuyObject {
                    group_id: buy.agent_data.group_id,
                    category_id: buy.agent_data.category_id,
                    objects: buy
                        .object_data
                        .iter()
                        .map(|item| ObjectBuyItem {
                            local_id: item.object_local_id,
                            sale_type: SaleType::from_code(item.sale_type),
                            sale_price: item.sale_price,
                        })
                        .collect(),
                });
            }
            AnyMessage::BuyObjectInventory(buy) => {
                self.events.push_back(ServerEvent::BuyObjectInventory {
                    object_id: buy.data.object_id,
                    item_id: buy.data.item_id,
                    folder_id: buy.data.folder_id,
                });
            }
            AnyMessage::RequestPayPrice(request) => {
                self.events.push_back(ServerEvent::RequestPayPrice {
                    object_id: request.object_data.object_id,
                });
            }
            AnyMessage::RequestObjectPropertiesFamily(request) => {
                self.events
                    .push_back(ServerEvent::RequestObjectPropertiesFamily {
                        request_flags: request.object_data.request_flags,
                        object_id: request.object_data.object_id,
                    });
            }
            AnyMessage::ObjectSpinStart(spin) => {
                self.events.push_back(ServerEvent::SpinObjectStart {
                    object_id: spin.object_data.object_id,
                });
            }
            AnyMessage::ObjectSpinUpdate(spin) => {
                self.events.push_back(ServerEvent::SpinObjectUpdate {
                    object_id: spin.object_data.object_id,
                    rotation: spin.object_data.rotation.clone(),
                });
            }
            AnyMessage::ObjectSpinStop(spin) => {
                self.events.push_back(ServerEvent::SpinObjectStop {
                    object_id: spin.object_data.object_id,
                });
            }
            AnyMessage::ObjectDuplicateOnRay(dup) => {
                let agent = &dup.agent_data;
                self.events.push_back(ServerEvent::DuplicateObjectsOnRay {
                    local_ids: dup
                        .object_data
                        .iter()
                        .map(|item| item.object_local_id)
                        .collect(),
                    group_id: agent.group_id,
                    ray_start: agent.ray_start.clone(),
                    ray_end: agent.ray_end.clone(),
                    bypass_raycast: agent.bypass_raycast,
                    ray_end_is_intersection: agent.ray_end_is_intersection,
                    copy_centers: agent.copy_centers,
                    copy_rotates: agent.copy_rotates,
                    ray_target_id: agent.ray_target_id,
                    duplicate_flags: agent.duplicate_flags,
                });
            }
            AnyMessage::RezRestoreToWorld(restore) => {
                let data = &restore.inventory_data;
                self.events.push_back(ServerEvent::RezRestoreToWorld {
                    item: RestoreItem {
                        item_id: data.item_id,
                        folder_id: data.folder_id,
                        creator_id: data.creator_id,
                        owner_id: data.owner_id,
                        group_id: data.group_id,
                        base_mask: data.base_mask,
                        owner_mask: data.owner_mask,
                        group_mask: data.group_mask,
                        everyone_mask: data.everyone_mask,
                        next_owner_mask: data.next_owner_mask,
                        group_owned: data.group_owned,
                        transaction_id: data.transaction_id,
                        asset_type: data.r#type,
                        inv_type: data.inv_type,
                        flags: data.flags,
                        sale_type: SaleType::from_code(data.sale_type),
                        sale_price: data.sale_price,
                        name: trimmed_string(&data.name),
                        description: trimmed_string(&data.description),
                        creation_date: data.creation_date,
                        crc: data.crc,
                    },
                });
            }
            AnyMessage::RezObjectFromNotecard(rez) => {
                let rez_data = &rez.rez_data;
                self.events.push_back(ServerEvent::RezObjectFromNotecard {
                    rez: NotecardRez {
                        group_id: rez.agent_data.group_id,
                        from_task_id: rez_data.from_task_id,
                        bypass_raycast: rez_data.bypass_raycast != 0,
                        ray_start: rez_data.ray_start.clone(),
                        ray_end: rez_data.ray_end.clone(),
                        ray_target_id: rez_data.ray_target_id,
                        ray_end_is_intersection: rez_data.ray_end_is_intersection,
                        rez_selected: rez_data.rez_selected,
                        remove_item: rez_data.remove_item,
                        item_flags: rez_data.item_flags,
                        group_mask: rez_data.group_mask,
                        everyone_mask: rez_data.everyone_mask,
                        next_owner_mask: rez_data.next_owner_mask,
                        notecard_item_id: rez.notecard_data.notecard_item_id,
                        object_id: rez.notecard_data.object_id,
                        item_ids: rez.inventory_data.iter().map(|item| item.item_id).collect(),
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
                        local_id: request.parcel_data.local_id,
                    });
            }
            AnyMessage::ParcelBuyPass(pass) => {
                self.events.push_back(ServerEvent::BuyParcelPass {
                    local_id: pass.parcel_data.local_id,
                });
            }
            AnyMessage::ParcelDisableObjects(disable) => {
                self.events.push_back(ServerEvent::DisableParcelObjects {
                    local_id: disable.parcel_data.local_id,
                    return_type: disable.parcel_data.return_type,
                    owner_ids: disable
                        .owner_i_ds
                        .iter()
                        .map(|owner| owner.owner_id)
                        .collect(),
                    task_ids: disable.task_i_ds.iter().map(|task| task.task_id).collect(),
                });
            }
            AnyMessage::ParcelInfoRequest(request) => {
                self.events.push_back(ServerEvent::RequestParcelInfo {
                    parcel_id: request.data.parcel_id,
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
                agent_id: self.agent_id.unwrap_or_else(Uuid::nil),
                session_id: self.session_id.unwrap_or_else(Uuid::nil),
            },
            data: AgentMovementCompleteDataBlock {
                position: Camera::region_center().center,
                look_at: Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
                region_handle: self.region_handle,
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
                agent_id: self.agent_id.unwrap_or_else(Uuid::nil),
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
            object_local_id: param1(),
        },
        "delete" => ServerEvent::DisconnectTelehub,
        "spawnpoint add" => ServerEvent::AddTelehubSpawnPoint {
            object_local_id: param1(),
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
