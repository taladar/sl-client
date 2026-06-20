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
    AgentMovementCompleteSimDataBlock, ChatFromSimulator, ChatFromSimulatorChatDataBlock,
    CompletePingCheck, CompletePingCheckPingIDBlock, LogoutReply, LogoutReplyAgentDataBlock,
    PacketAck, StartPingCheck, StartPingCheckPingIDBlock, UUIDGroupNameReply,
    UUIDGroupNameReplyUUIDNameBlockBlock, UUIDNameReply, UUIDNameReplyUUIDNameBlockBlock,
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
use crate::types::{
    AvatarName, Camera, ChatType, GroupName, InstantMessage, MapItem, MapItemType, MapRegionInfo,
    RegionIdentity, Reliability, Throttle, Transmit,
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
