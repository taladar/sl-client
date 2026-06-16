//! The sans-I/O session state machine: login, circuit establishment,
//! keep-alive, and clean logout, driven entirely by passed-in time.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use sl_types::lsl::{Rotation, Vector};
use sl_wire::messages::{
    AgentUpdate, AgentUpdateAgentDataBlock, ChatFromSimulatorChatDataBlock, ChatFromViewer,
    ChatFromViewerAgentDataBlock, ChatFromViewerChatDataBlock, CompleteAgentMovement,
    CompleteAgentMovementAgentDataBlock, CompletePingCheck, CompletePingCheckPingIDBlock,
    EnableSimulatorSimulatorInfoBlock, LogoutRequest, LogoutRequestAgentDataBlock,
    MapBlockReplyDataBlock, MapBlockReplySizeBlock, MapBlockRequest, MapBlockRequestAgentDataBlock,
    MapBlockRequestPositionDataBlock, PacketAck, PacketAckPacketsBlock,
    ParcelPropertiesParcelDataBlock, ParcelPropertiesRequest,
    ParcelPropertiesRequestAgentDataBlock, ParcelPropertiesRequestParcelDataBlock,
    RegionHandshakeRegionInfo3Block, RegionHandshakeRegionInfoBlock, RegionHandshakeReply,
    RegionHandshakeReplyAgentDataBlock, RegionHandshakeReplyRegionInfoBlock,
    RegionInfoRegionInfo2Block, RegionInfoRegionInfoBlock, RequestRegionInfo,
    RequestRegionInfoAgentDataBlock, TeleportLocationRequest,
    TeleportLocationRequestAgentDataBlock, TeleportLocationRequestInfoBlock, UseCircuitCode,
    UseCircuitCodeCircuitCodeBlock,
};
use sl_wire::{
    AnyMessage, Llsd, MessageId, PacketFlags, Reader, WireError, Writer, build_login_request,
    encode_datagram, parse_datagram, zero_decode,
};
use uuid::Uuid;

use crate::error::Error;
use crate::types::{
    ChatAudible, ChatMessage, ChatSourceType, ChatType, DisconnectReason, Event, LoginHttpRequest,
    LoginParams, MapRegionInfo, Maturity, NeighborInfo, ParcelInfo, ParcelOverlayInfo, ProductType,
    RegionIdentity, RegionLimits, Reliability, Transmit, grid_to_handle, handle_to_grid,
};

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

    /// Queues a keep-alive `AgentUpdate` unreliably.
    ///
    /// The camera is placed at the region centre with an orthonormal basis and
    /// the configured draw distance, so the simulator builds an interest list
    /// and enables the neighbouring regions (which arrive as `EnableSimulator`).
    fn send_agent_update(&mut self, now: Instant) -> Result<(), WireError> {
        let identity = Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        };
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
                body_rotation: identity.clone(),
                head_rotation: identity,
                state: 0,
                camera_center,
                camera_at_axis,
                camera_left_axis,
                camera_up_axis,
                far: self.draw_distance,
                control_flags: 0,
                flags: 0,
            },
        });
        self.send(&message, Reliability::Unreliable, now)
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
    /// The active circuit, once login has succeeded.
    circuit: Option<Circuit>,
    /// The draw distance (metres) advertised in keep-alive `AgentUpdate`s.
    draw_distance: f32,
    /// In-progress teleport handover bookkeeping, if any.
    handover: Option<HandoverPending>,
    /// The destination region handle of an in-flight teleport (between sending
    /// `TeleportLocationRequest` and receiving `TeleportFinish`/failure).
    teleport_target: Option<u64>,
    /// The current region's capability-seed URL (from login or a teleport), for
    /// the driver to fetch the CAPS map and event queue.
    seed_capability: Option<String>,
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
            draw_distance: DEFAULT_DRAW_DISTANCE,
            handover: None,
            teleport_target: None,
            seed_capability: None,
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

    /// Feeds a parsed CAPS `EventQueue` event into the session, surfacing any
    /// recognised payload. Handles `ParcelProperties` (delivered over the event
    /// queue, not UDP) and `TeleportFinish` (the destination address for a
    /// cross-region teleport — OpenSim sends this over the event queue, not UDP).
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
                self.seed_capability = Some(success.seed_capability.clone());
                self.state = SessionState::AwaitingHandshake;
                self.events
                    .push_back(Event::CircuitEstablished { sim: sim_addr });
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
        // Only accept traffic from the simulator we are connected to.
        if self.circuit.as_ref().map(|c| c.sim_addr) != Some(from) {
            return Ok(());
        }

        let parsed = parse_datagram(datagram)?;

        let process = {
            let Some(circuit) = self.circuit.as_mut() else {
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
        self.dispatch(&message, now)
    }

    /// Acts on a decoded inbound message.
    fn dispatch(&mut self, message: &AnyMessage, now: Instant) -> Result<(), Error> {
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
            AnyMessage::EnableSimulator(sim) => {
                self.events
                    .push_back(Event::NeighborDiscovered(neighbor_info(
                        &sim.simulator_info,
                    )));
            }
            AnyMessage::MapBlockReply(reply) => {
                for (index, data) in reply.data.iter().enumerate() {
                    if let Some(region) = map_region_info(data, reply.size.get(index)) {
                        self.events.push_back(Event::MapBlock(Box::new(region)));
                    }
                }
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
            && let Some(circuit) = self.circuit.as_mut()
        {
            circuit.send_agent_update(now)?;
            circuit.timers.agent_update = Some(deadline(now, AGENT_UPDATE_INTERVAL));
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

    /// The next datagram to transmit, if any.
    pub fn poll_transmit(&mut self) -> Option<Transmit> {
        let circuit = self.circuit.as_mut()?;
        let payload = circuit.out.pop_front()?;
        Some(Transmit {
            destination: circuit.sim_addr,
            payload,
        })
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
        raw_parcel_flags: data
            .get("ParcelFlags")
            .and_then(Llsd::as_i32)
            .unwrap_or(0)
            .cast_unsigned(),
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
