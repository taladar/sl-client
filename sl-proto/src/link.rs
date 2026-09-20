//! The LLUDP reliable-transport link, owned by both directions of the protocol.
//!
//! LLUDP layers reliability over UDP with four pieces of per-connection
//! bookkeeping: an outgoing sequence counter, a map of reliable packets still
//! awaiting acknowledgement, a batch of acknowledgements owed to the peer, and
//! a bounded window of inbound reliable sequence numbers already seen (so a
//! retransmission is not processed twice). A round-trip measurement, taken from
//! the `StartPingCheck` / `CompletePingCheck` keep-alive exchange, sets how
//! long an unacknowledged packet waits before it is sent again.
//!
//! None of that is specific to a direction: a client circuit
//! ([`Circuit`](crate::session::Circuit)) and a simulator session
//! ([`SimSession`](crate::SimSession)) run the same protocol against each
//! other, and both used to carry their own copy of this layer — the same
//! structs, the same seven constants, the same six methods — with nothing
//! keeping the copies in step. They had already drifted once
//! (`protocol-audit-extract-lludp-transport`). This module is that layer, once:
//! both sessions hold a [`ReliableLink`] and keep only what is genuinely
//! theirs — which messages they send, and what a lost one costs them.
//!
//! What stays outside: the peer's address, the agent/session/circuit ids, and
//! every timer that is not the link's own (`AgentUpdate` cadence, logout,
//! teleport, sit). The link owns exactly the two deadlines that *are*
//! transport — when the link is declared dead for silence, and when owed
//! acknowledgements must go out.

use crate::ack_flush::send_ack_packets;
use crate::bookkeeping_ids::PingId;
use crate::types::Reliability;
use sl_wire::messages::{StartPingCheck, StartPingCheckPingIDBlock};
use sl_wire::{AnyMessage, PacketFlags, SequenceNumber, WireError, Writer, encode_datagram};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

/// How long owed acknowledgements may wait before being flushed as a
/// `PacketAck`.
pub(crate) const ACK_FLUSH_DELAY: Duration = Duration::from_millis(150);

/// How long a link may go without any inbound traffic before it is considered
/// dead. Kept well under OpenSim's 60-second `AckTimeout`.
pub(crate) const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(45);

/// The floor on the retransmission timeout for an unacknowledged reliable
/// packet, however fast the round trip is. Mirrors the reference viewer's
/// `LL_MINIMUM_RELIABLE_TIMEOUT_SECONDS`.
pub(crate) const MINIMUM_RESEND_TIMEOUT: Duration = Duration::from_secs(1);

/// The multiple of the link's averaged round-trip time used as the
/// retransmission timeout (the reference viewer's
/// `LL_RELIABLE_TIMEOUT_FACTOR`), floored at [`MINIMUM_RESEND_TIMEOUT`].
pub(crate) const RELIABLE_TIMEOUT_FACTOR: f32 = 5.0;

/// The weight a fresh round-trip sample carries in the ping average (the
/// reference viewer's `LL_AVERAGED_PING_ALPHA`).
const PING_AVERAGE_ALPHA: f32 = 0.2;

/// The weight the previous ping average keeps when a fresh sample arrives —
/// `1.0 - PING_AVERAGE_ALPHA`, spelled out so the update is literal-only
/// arithmetic.
const PING_AVERAGE_DECAY: f32 = 0.8;

/// The floor the averaged round-trip time is clamped to (the reference
/// viewer's `LL_AVERAGED_PING_MIN`), keeping a very fast link from driving the
/// retransmission timeout below what a briefly busy peer needs.
const PING_AVERAGE_MIN: Duration = Duration::from_millis(100);

/// The ceiling the averaged round-trip time is clamped to (the reference
/// viewer's `LL_AVERAGED_PING_MAX`), bounding the retransmission timeout at
/// [`RELIABLE_TIMEOUT_FACTOR`] times this.
pub(crate) const PING_AVERAGE_MAX: Duration = Duration::from_millis(2000);

/// The round-trip time a link assumes before any keep-alive ping has been
/// answered (the reference viewer's `INITIAL_PING_VALUE_MSEC`).
const INITIAL_PING_AVERAGE: Duration = Duration::from_millis(1000);

/// How often a keep-alive `StartPingCheck` is sent to measure the round-trip
/// time to the peer, matching the reference viewer's circuit ping cadence
/// (`LLCircuit`'s ~5-second periodic ping).
pub(crate) const PING_INTERVAL: Duration = Duration::from_secs(5);

/// The maximum number of times a reliable packet is sent before giving up: the
/// first transmission plus the reference viewer's
/// `LL_DEFAULT_RELIABLE_RETRIES` retries.
pub(crate) const MAX_RESEND_ATTEMPTS: u32 = 4;

/// The maximum number of inbound reliable sequence numbers remembered for
/// duplicate suppression.
const SEEN_CAPACITY: usize = 4096;

/// Computes `now + duration`, saturating at `now` on (impossible) overflow.
///
/// Lives here because every deadline in either session is one of these, and the
/// link's own two are the first of them.
pub(crate) fn deadline(now: Instant, duration: Duration) -> Instant {
    now.checked_add(duration).unwrap_or(now)
}

/// Updates `earliest` to the minimum of itself and `candidate`.
pub(crate) fn merge_deadline(earliest: &mut Option<Instant>, candidate: Option<Instant>) {
    if let Some(candidate) = candidate {
        *earliest = Some(match *earliest {
            Some(current) => current.min(candidate),
            None => candidate,
        });
    }
}

/// What losing a reliable packet for good costs the session holding the link.
///
/// Which messages fall in which class is the one part of the reliable layer
/// that *is* direction-specific — the client's session-critical packets are
/// the ones that get the agent admitted, the simulator's are the ones that get
/// it arrived — so each session classifies its own outgoing messages and passes
/// the answer to [`ReliableLink::send`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReliableSeverity {
    /// The packet establishes the session itself. Without it the peers never
    /// agree that the agent is present, so there is nothing left to keep the
    /// link open for and exhausting its retransmissions fails the session.
    SessionCritical,
    /// An ordinary reliable message (chat, a selection, an inventory request).
    /// Losing it costs that one action; the session keeps running and the loss
    /// is surfaced to the driver. This matches the reference viewer, where an
    /// exhausted reliable packet only invokes its per-packet failure callback
    /// (`LL_ERR_TCP_TIMEOUT`) and leaves the circuit alone — a dead link is
    /// detected by the inactivity timeout, not by one lost message.
    BestEffort,
}

/// A reliable packet that has run out of retransmissions, reported by
/// [`ReliableLink::process_resends`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct ExhaustedPacket {
    /// The outgoing sequence number the packet was sent with.
    pub(crate) sequence: SequenceNumber,
    /// The message name, for the give-up report (`None` for an unrecognised
    /// id).
    pub(crate) name: Option<&'static str>,
    /// What the loss costs the session.
    pub(crate) severity: ReliableSeverity,
}

/// A datagram queued for transmission on the link.
#[derive(Debug, Clone)]
struct Outbound {
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
    /// datagram is still `queued`, this is pushed forward to the latest instant
    /// the session is told about, so time the datagram spends waiting on a
    /// backed-up driver does not count against its timeout — the clock only
    /// really starts once the datagram has left the host.
    sent_at: Instant,
    /// Whether the current attempt's datagram is still sitting in the outbound
    /// queue rather than having been handed to the driver.
    queued: bool,
    /// How many times the packet has been sent so far.
    attempts: u32,
    /// The message name, used to label the give-up report when the packet
    /// exhausts its retransmission budget (`None` for an unrecognised id).
    name: Option<&'static str>,
    /// What losing this packet for good costs the session.
    severity: ReliableSeverity,
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

/// The reliable-transport half of a connection: sequence numbering, the
/// unacknowledged set and its resend policy, owed acknowledgements, duplicate
/// suppression, the outbound datagram queue, and the keep-alive ping that
/// measures the round trip they all depend on.
#[derive(Debug)]
pub(crate) struct ReliableLink {
    /// The next outgoing sequence number.
    next_sequence: SequenceNumber,
    /// The next outgoing keep-alive ping id (mirrors the reference viewer's
    /// `LLCircuitData::mLastPingID`); a wrapping `u8` the matching
    /// `CompletePingCheck` echoes back.
    next_ping_id: PingId,
    /// The in-flight keep-alive ping awaiting its `CompletePingCheck`, paired
    /// with the instant it was sent so the round-trip time can be measured.
    /// `None` when no ping is outstanding.
    outstanding_ping: Option<(PingId, Instant)>,
    /// The fast-attack / slow-decay average of the measured round-trip time
    /// (the reference viewer's `mPingDelayAveraged`), clamped to
    /// `PING_AVERAGE_MIN ..= PING_AVERAGE_MAX`. Drives the retransmission
    /// timeout, so a slow or congested link waits longer before resending
    /// instead of piling retransmissions onto it.
    ping_average: Duration,
    /// Inbound reliable sequence numbers we still owe acknowledgements for.
    pending_acks: Vec<SequenceNumber>,
    /// Outgoing reliable packets awaiting acknowledgement, keyed by sequence.
    unacked: BTreeMap<SequenceNumber, UnackedPacket>,
    /// Recently seen inbound reliable sequence numbers.
    seen: SeenWindow,
    /// Datagrams ready to be transmitted.
    out: VecDeque<Outbound>,
    /// When the link is declared dead for lack of inbound traffic.
    inactivity: Instant,
    /// When to flush owed acknowledgements, if any are pending.
    ack_flush: Option<Instant>,
}

impl ReliableLink {
    /// Creates a link and arms its inactivity timer.
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            next_sequence: SequenceNumber::FIRST,
            next_ping_id: PingId::default(),
            outstanding_ping: None,
            ping_average: INITIAL_PING_AVERAGE,
            pending_acks: Vec::new(),
            unacked: BTreeMap::new(),
            seen: SeenWindow::default(),
            out: VecDeque::new(),
            inactivity: deadline(now, INACTIVITY_TIMEOUT),
            ack_flush: None,
        }
    }

    /// The sequence number the next packet sent on this link will carry,
    /// without allocating it.
    pub(crate) const fn peek_next_sequence(&self) -> SequenceNumber {
        self.next_sequence
    }

    /// Allocates the next outgoing sequence number.
    const fn next_sequence(&mut self) -> SequenceNumber {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_next();
        sequence
    }

    /// Encodes and queues a message, tracking it for resend when reliable.
    ///
    /// `severity` is what the caller's session loses if the packet exhausts its
    /// retransmission budget; it is carried on the packet and reported back by
    /// [`Self::process_resends`].
    ///
    /// # Errors
    ///
    /// Returns a wire error if the message fails to encode.
    pub(crate) fn send(
        &mut self,
        message: &AnyMessage,
        reliability: Reliability,
        severity: ReliableSeverity,
        now: Instant,
    ) -> Result<(), WireError> {
        let mut writer = Writer::new();
        message.id().encode(&mut writer)?;
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
                    severity,
                },
            );
            Some(sequence)
        } else {
            None
        };
        self.out.push_back(Outbound {
            sequence: tracked,
            payload: datagram,
        });
        Ok(())
    }

    /// Pops the next datagram to hand to the driver, starting the
    /// retransmission clock of the reliable packet it carries: until now that
    /// packet was only *queued*, and time spent in the queue must not count
    /// against its timeout (see [`UnackedPacket::sent_at`]).
    pub(crate) fn pop_outbound(&mut self) -> Option<Vec<u8>> {
        let outbound = self.out.pop_front()?;
        if let Some(sequence) = outbound.sequence
            && let Some(packet) = self.unacked.get_mut(&sequence)
        {
            packet.queued = false;
        }
        Some(outbound.payload)
    }

    /// Whether the reliable packet sent as `sequence` is still awaiting the
    /// peer's acknowledgement.
    ///
    /// `false` for a sequence this link never sent, for one already
    /// acknowledged, and for an unreliable packet — which is never tracked, and
    /// so is never waited for.
    pub(crate) fn is_awaiting_ack(&self, sequence: SequenceNumber) -> bool {
        self.unacked.contains_key(&sequence)
    }

    /// Records that a datagram was received, resetting the inactivity timer.
    pub(crate) fn note_received(&mut self, now: Instant) {
        self.inactivity = deadline(now, INACTIVITY_TIMEOUT);
    }

    /// When the link is declared dead for lack of inbound traffic.
    pub(crate) const fn inactivity_deadline(&self) -> Instant {
        self.inactivity
    }

    /// When owed acknowledgements are due to be flushed, if any are pending.
    pub(crate) const fn ack_flush_deadline(&self) -> Option<Instant> {
        self.ack_flush
    }

    /// Records that we owe an acknowledgement for `sequence`, arming the flush.
    pub(crate) fn queue_ack(&mut self, sequence: SequenceNumber, now: Instant) {
        self.pending_acks.push(sequence);
        if self.ack_flush.is_none() {
            self.ack_flush = Some(deadline(now, ACK_FLUSH_DELAY));
        }
    }

    /// Removes the given outgoing sequence numbers from the unacked set.
    pub(crate) fn record_acks(&mut self, ids: &[SequenceNumber]) {
        for id in ids {
            self.unacked.remove(id);
        }
    }

    /// Records an inbound reliable `sequence`; returns `true` if it is new.
    pub(crate) fn mark_seen(&mut self, sequence: SequenceNumber) -> bool {
        self.seen.insert(sequence)
    }

    /// Flushes owed acknowledgements as one or more `PacketAck` messages.
    ///
    /// A message that fails to encode does not take the acks batched behind it
    /// with it — see [`send_ack_packets`] for why every message is sent even
    /// after one fails, and why the first failure is the one returned.
    ///
    /// # Errors
    ///
    /// Returns the first wire error a `PacketAck` failed to encode with.
    pub(crate) fn flush_acks(&mut self, now: Instant) -> Result<(), WireError> {
        self.ack_flush = None;
        if self.pending_acks.is_empty() {
            return Ok(());
        }
        let acks = std::mem::take(&mut self.pending_acks);
        send_ack_packets(&acks, |message| {
            self.send(
                message,
                Reliability::Unreliable,
                ReliableSeverity::BestEffort,
                now,
            )
        })
    }

    /// The retransmission timeout for this link: the reference viewer's
    /// `LL_RELIABLE_TIMEOUT_FACTOR` multiple of the averaged round-trip time,
    /// floored at [`MINIMUM_RESEND_TIMEOUT`].
    fn resend_timeout(&self) -> Duration {
        self.ping_average
            .mul_f32(RELIABLE_TIMEOUT_FACTOR)
            .max(MINIMUM_RESEND_TIMEOUT)
    }

    /// Folds a round-trip `sample` into the ping average with the reference
    /// viewer's fast-attack / slow-decay relaxation
    /// (`LLCircuitData::setPingDelay`): the average first jumps to any worse
    /// sample, then relaxes toward it, and the result is clamped to
    /// `PING_AVERAGE_MIN ..= PING_AVERAGE_MAX`.
    pub(crate) fn record_ping_sample(&mut self, sample: Duration) {
        let attacked = self.ping_average.max(sample);
        self.ping_average = attacked
            .mul_f32(PING_AVERAGE_DECAY)
            .saturating_add(sample.mul_f32(PING_AVERAGE_ALPHA))
            .clamp(PING_AVERAGE_MIN, PING_AVERAGE_MAX);
    }

    /// Queues a keep-alive `StartPingCheck` unreliably, recording it as the
    /// outstanding ping so the matching `CompletePingCheck` can be timed.
    /// Returns the ping id sent.
    ///
    /// Like the reference viewer, the ping carries this end's oldest unacked
    /// outgoing sequence number in `OldestUnacked`, letting the peer drop its
    /// own duplicate-suppression record of anything older. "Oldest" is read off
    /// the wrapping counter rather than the numeric order of the set — see
    /// [`unacked::oldest`](crate::unacked::oldest).
    ///
    /// # Errors
    ///
    /// Returns a wire error if the message fails to encode.
    pub(crate) fn send_start_ping_check(&mut self, now: Instant) -> Result<PingId, WireError> {
        // A ping still outstanding when the next one is due is itself evidence
        // about the link: the round trip is at least the time it has been in
        // flight. Folding that in is this link's form of the reference viewer's
        // `getPingInTransitTime`, which inflates the averaged ping while pings
        // go unanswered — so a peer that has stopped replying stretches the
        // retransmission timeout instead of drawing ever more retransmissions
        // onto an already struggling link.
        if let Some((_, sent_at)) = self.outstanding_ping {
            self.record_ping_sample(now.saturating_duration_since(sent_at));
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
        self.send(
            &message,
            Reliability::Unreliable,
            ReliableSeverity::BestEffort,
            now,
        )?;
        self.outstanding_ping = Some((ping_id, now));
        Ok(ping_id)
    }

    /// Records an inbound `CompletePingCheck`, returning the round-trip time
    /// when it answers the outstanding keep-alive ping.
    ///
    /// Returns `None` for an unsolicited reply or one whose id does not match
    /// the ping in flight (a stale or duplicate echo), leaving any genuine
    /// outstanding ping untouched.
    pub(crate) fn record_ping_reply(&mut self, ping_id: PingId, now: Instant) -> Option<Duration> {
        match self.outstanding_ping {
            Some((outstanding, sent_at)) if outstanding == ping_id => {
                self.outstanding_ping = None;
                let round_trip = now.saturating_duration_since(sent_at);
                self.record_ping_sample(round_trip);
                Some(round_trip)
            }
            _ => None,
        }
    }

    /// Retransmits unacknowledged reliable packets whose timeout has elapsed.
    ///
    /// The timeout tracks the measured round trip ([`Self::resend_timeout`]),
    /// and a datagram still waiting in the outbound queue has its clock held at
    /// `now` rather than counting the wait as silence from the peer — so a
    /// driver that falls behind does not turn its own backlog into a burst of
    /// retransmissions.
    ///
    /// Returns every packet that has now exhausted its retransmission budget;
    /// such packets are dropped from the unacked set (so they are reported only
    /// once and stop driving the resend deadline). An empty result means
    /// nothing exhausted this tick.
    pub(crate) fn process_resends(&mut self, now: Instant) -> Vec<ExhaustedPacket> {
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
            to_send.push(Outbound {
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

    /// The earliest retransmission deadline across all unacked packets. A
    /// packet whose datagram is still queued has not started its clock, so it
    /// does not contribute a deadline — its wake-up comes from the transmission
    /// itself.
    pub(crate) fn next_resend_deadline(&self) -> Option<Instant> {
        let timeout = self.resend_timeout();
        self.unacked
            .values()
            .filter(|packet| !packet.queued)
            .map(|packet| deadline(packet.sent_at, timeout))
            .min()
    }

    /// Drops everything the link was waiting on: owed acknowledgements, the
    /// unacknowledged set, the outstanding ping and the ack-flush deadline.
    ///
    /// The outbound queue is deliberately left alone — a session that closes
    /// still has to drain the goodbye packet it queued on its way out.
    pub(crate) fn quiesce(&mut self) {
        self.ack_flush = None;
        self.outstanding_ping = None;
        self.pending_acks = Vec::new();
        self.unacked = BTreeMap::new();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        INACTIVITY_TIMEOUT, INITIAL_PING_AVERAGE, MAX_RESEND_ATTEMPTS, MINIMUM_RESEND_TIMEOUT,
        RELIABLE_TIMEOUT_FACTOR, ReliableLink, ReliableSeverity, deadline,
    };
    use crate::bookkeeping_ids::PingId;
    use crate::types::Reliability;
    use pretty_assertions::assert_eq;
    use sl_wire::messages::PacketAck;
    use sl_wire::{AnyMessage, MessageId, PacketFlags, Reader, SequenceNumber};
    use std::time::{Duration, Instant};

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// An empty `PacketAck`, the smallest message that encodes — the tests here
    /// care about the framing and the bookkeeping, never the body.
    fn message() -> AnyMessage {
        AnyMessage::PacketAck(PacketAck {
            packets: Vec::new(),
        })
    }

    /// The message a queued datagram carries. None of these tests queues a
    /// zerocoded packet, so the body is read straight out of the framing.
    fn decode(datagram: &[u8]) -> Result<AnyMessage, TestError> {
        let parsed = sl_wire::parse_datagram(datagram)?;
        let mut reader = Reader::new(parsed.body);
        let id = MessageId::decode(&mut reader)?;
        Ok(AnyMessage::decode(id, &mut reader)?)
    }

    /// Whether a queued datagram's prelude carries the `RESENT` flag.
    fn is_resent(datagram: &[u8]) -> Result<bool, TestError> {
        let first = datagram.first().ok_or("a datagram is never empty")?;
        Ok(PacketFlags::from_bits(*first).contains(PacketFlags::RESENT))
    }

    /// A reliable send is tracked until its acknowledgement arrives, and an
    /// unreliable one is never tracked at all.
    #[test]
    fn only_reliable_packets_are_tracked() -> Result<(), TestError> {
        let now = Instant::now();
        let mut link = ReliableLink::new(now);

        link.send(
            &message(),
            Reliability::Unreliable,
            ReliableSeverity::BestEffort,
            now,
        )?;
        assert!(
            !link.is_awaiting_ack(SequenceNumber::FIRST),
            "an unreliable packet is never waited for"
        );

        let sequence = link.peek_next_sequence();
        link.send(
            &message(),
            Reliability::Reliable,
            ReliableSeverity::BestEffort,
            now,
        )?;
        assert!(link.is_awaiting_ack(sequence));
        link.record_acks(&[sequence]);
        assert!(!link.is_awaiting_ack(sequence));
        Ok(())
    }

    /// A reliable packet is resent until its budget runs out, and is then
    /// reported once — with the severity its sender gave it — and dropped.
    #[test]
    fn a_reliable_packet_exhausts_its_budget_once() -> Result<(), TestError> {
        let start = Instant::now();
        let mut link = ReliableLink::new(start);
        let sequence = link.peek_next_sequence();
        link.send(
            &message(),
            Reliability::Reliable,
            ReliableSeverity::SessionCritical,
            start,
        )?;

        // What the link waits before resending, with no round trip measured
        // yet: the starting ping average through `resend_timeout`'s policy.
        let timeout = INITIAL_PING_AVERAGE
            .mul_f32(RELIABLE_TIMEOUT_FACTOR)
            .max(MINIMUM_RESEND_TIMEOUT);

        // Until the datagram leaves the queue its clock has not started, so no
        // amount of time resends it.
        let mut now = deadline(start, Duration::from_secs(60));
        assert!(link.process_resends(now).is_empty());
        assert_eq!(link.next_resend_deadline(), None);
        let datagram = link.pop_outbound().ok_or("the first attempt is queued")?;
        assert!(!is_resent(&datagram)?, "the first attempt is not a resend");
        assert_eq!(
            link.next_resend_deadline(),
            Some(deadline(now, timeout)),
            "popping the datagram starts its retransmission clock"
        );

        // Each timed-out tick resends once, up to the retry budget.
        for attempt in 1..MAX_RESEND_ATTEMPTS {
            now = deadline(now, timeout);
            assert!(
                link.process_resends(now).is_empty(),
                "attempt {attempt} still has budget left"
            );
            let resent = link.pop_outbound().ok_or("a retransmission is queued")?;
            assert!(is_resent(&resent)?, "a retransmission is flagged RESENT");
        }

        now = deadline(now, timeout);
        let exhausted = link.process_resends(now);
        assert_eq!(exhausted.len(), 1);
        let packet = exhausted.first().ok_or("one packet gave up")?;
        assert_eq!(packet.sequence, sequence);
        assert_eq!(packet.severity, ReliableSeverity::SessionCritical);
        assert_eq!(packet.name, Some("PacketAck"));

        // Reported once: the packet is gone from the unacked set, so a later
        // tick has nothing left to give up on.
        assert!(!link.is_awaiting_ack(sequence));
        now = deadline(now, timeout);
        assert!(link.process_resends(now).is_empty());
        Ok(())
    }

    /// Owed acknowledgements arm one flush deadline and go out as a `PacketAck`
    /// when it is reached; a duplicate inbound sequence is suppressed.
    #[test]
    fn owed_acks_flush_and_duplicates_are_suppressed() -> Result<(), TestError> {
        let now = Instant::now();
        let mut link = ReliableLink::new(now);
        assert_eq!(link.ack_flush_deadline(), None);

        let inbound = SequenceNumber::new(7);
        assert!(link.mark_seen(inbound), "first sight of a sequence is new");
        assert!(!link.mark_seen(inbound), "a retransmission is suppressed");
        link.queue_ack(inbound, now);
        let armed = link.ack_flush_deadline().ok_or("the flush is armed")?;
        link.queue_ack(SequenceNumber::new(8), now);
        assert_eq!(
            link.ack_flush_deadline(),
            Some(armed),
            "a second owed ack joins the batch rather than re-arming it"
        );

        link.flush_acks(armed)?;
        assert_eq!(link.ack_flush_deadline(), None);
        let datagram = link.pop_outbound().ok_or("the PacketAck is queued")?;
        match decode(&datagram)? {
            AnyMessage::PacketAck(ack) => {
                let ids: Vec<u32> = ack.packets.iter().map(|packet| packet.id).collect();
                assert_eq!(ids, vec![7, 8]);
            }
            other => return Err(format!("expected a PacketAck, got {other:?}").into()),
        }
        Ok(())
    }

    /// Inbound traffic pushes the inactivity deadline out, and the keep-alive
    /// ping's round trip is folded into the average its reply measures.
    #[test]
    fn a_ping_reply_measures_the_round_trip() -> Result<(), TestError> {
        let start = Instant::now();
        let mut link = ReliableLink::new(start);
        assert_eq!(
            link.inactivity_deadline(),
            deadline(start, INACTIVITY_TIMEOUT)
        );
        let later = deadline(start, Duration::from_secs(10));
        link.note_received(later);
        assert_eq!(
            link.inactivity_deadline(),
            deadline(later, INACTIVITY_TIMEOUT)
        );

        let counter = link.peek_next_sequence();
        let ping_id = link.send_start_ping_check(later)?;
        let datagram = link.pop_outbound().ok_or("the ping is queued")?;
        match decode(&datagram)? {
            AnyMessage::StartPingCheck(ping) => {
                assert_eq!(ping.ping_id.ping_id, ping_id.get());
                assert_eq!(
                    ping.ping_id.oldest_unacked,
                    counter.get(),
                    "with nothing outstanding the ping reports the counter itself"
                );
            }
            other => return Err(format!("expected a StartPingCheck, got {other:?}").into()),
        }

        let reply_at = deadline(later, Duration::from_millis(250));
        assert_eq!(
            link.record_ping_reply(PingId(ping_id.get().wrapping_add(1)), reply_at),
            None,
            "a reply to another ping id is stale"
        );
        assert_eq!(
            link.record_ping_reply(ping_id, reply_at),
            Some(Duration::from_millis(250))
        );
        assert_eq!(
            link.record_ping_reply(ping_id, reply_at),
            None,
            "the answered ping is no longer outstanding"
        );
        Ok(())
    }

    /// Closing a session drops what the link was waiting on but keeps the
    /// datagrams already queued, so a goodbye packet still reaches the peer.
    #[test]
    fn quiesce_keeps_the_outbound_queue() -> Result<(), TestError> {
        let now = Instant::now();
        let mut link = ReliableLink::new(now);
        let sequence = link.peek_next_sequence();
        link.send(
            &message(),
            Reliability::Reliable,
            ReliableSeverity::BestEffort,
            now,
        )?;
        link.queue_ack(SequenceNumber::new(3), now);

        link.quiesce();
        assert!(!link.is_awaiting_ack(sequence));
        assert_eq!(link.ack_flush_deadline(), None);
        assert!(
            link.pop_outbound().is_some(),
            "the queued goodbye datagram still drains"
        );
        Ok(())
    }
}
