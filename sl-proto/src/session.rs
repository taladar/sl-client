//! The sans-I/O session state machine: login, circuit establishment,
//! keep-alive, and clean logout, driven entirely by passed-in time.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use sl_types::lsl::{Rotation, Vector};
use sl_wire::messages::{
    AgentUpdate, AgentUpdateAgentDataBlock, CompleteAgentMovement,
    CompleteAgentMovementAgentDataBlock, CompletePingCheck, CompletePingCheckPingIDBlock,
    LogoutRequest, LogoutRequestAgentDataBlock, PacketAck, PacketAckPacketsBlock,
    RegionHandshakeReply, RegionHandshakeReplyAgentDataBlock, RegionHandshakeReplyRegionInfoBlock,
    UseCircuitCode, UseCircuitCodeCircuitCodeBlock,
};
use sl_wire::{
    AnyMessage, MessageId, PacketFlags, Reader, WireError, Writer, build_login_request,
    encode_datagram, parse_datagram, zero_decode,
};
use uuid::Uuid;

use crate::error::Error;
use crate::types::{DisconnectReason, Event, LoginHttpRequest, LoginParams, Reliability, Transmit};

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
            timers: Timers {
                inactivity: deadline(now, INACTIVITY_TIMEOUT),
                ack_flush: None,
                agent_update: None,
                logout: None,
            },
        }
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

    /// Queues a minimal `AgentUpdate` keep-alive unreliably.
    fn send_agent_update(&mut self, now: Instant) -> Result<(), WireError> {
        let identity = Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        };
        let zero = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let message = AnyMessage::AgentUpdate(AgentUpdate {
            agent_data: AgentUpdateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                body_rotation: identity.clone(),
                head_rotation: identity,
                state: 0,
                camera_center: zero.clone(),
                camera_at_axis: zero.clone(),
                camera_left_axis: zero.clone(),
                camera_up_axis: zero,
                far: 128.0,
                control_flags: 0,
                flags: 0,
            },
        });
        self.send(&message, Reliability::Unreliable, now)
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
    /// A `LogoutRequest` was sent; awaiting the `LogoutReply`.
    LoggingOut,
    /// The session is finished.
    Closed,
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
            events: VecDeque::new(),
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
                    now,
                );
                circuit.send_use_circuit_code(now)?;
                circuit.send_complete_agent_movement(now)?;
                self.circuit = Some(circuit);
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
            AnyMessage::RegionHandshake(_) => {
                if matches!(self.state, SessionState::AwaitingHandshake) {
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.send_region_handshake_reply(now)?;
                        circuit.timers.agent_update = Some(deadline(now, AGENT_UPDATE_INTERVAL));
                    }
                    self.state = SessionState::Active;
                    self.events.push_back(Event::RegionHandshakeComplete);
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
