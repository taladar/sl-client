//! Packing owed acknowledgements into `PacketAck` messages.
//!
//! Both directions of the protocol owe the same debt: every reliable datagram
//! received has to be acknowledged, the acks are batched behind a short flush
//! timer, and the batch is then packed into as many `PacketAck` messages as it
//! takes. The client circuit ([`Circuit::flush_acks`](crate::session::Circuit))
//! and the simulator session ([`SimSession`](crate::SimSession)) share nothing
//! else about their transports, so the batching rule — and the policy for what
//! happens when one of those messages fails to encode — lives here, once.

use sl_wire::SequenceNumber;
use sl_wire::messages::{AnyMessage, PacketAck, PacketAckPacketsBlock};

/// The maximum number of acknowledgements packed into a single `PacketAck`.
///
/// The message's block count is a `u8`, so a longer batch would fail to encode
/// with [`WireError::VariableTooLong`](sl_wire::WireError::VariableTooLong).
pub(crate) const MAX_ACKS_PER_PACKET: usize = 255;

/// Packs `acks` into `PacketAck` messages of at most [`MAX_ACKS_PER_PACKET`]
/// ids each, oldest first, and hands each one to `send_one`.
///
/// Every message is offered to `send_one` even after an earlier one fails, and
/// the first error is returned once they all have. That is deliberate: an
/// encode failure here is a function of the message's own contents (only the
/// block count can overflow, which the chunking rules out), so it is
/// deterministic for that batch and re-queueing it would wedge every ack behind
/// it forever. The acks in the *other* messages are perfectly sendable, and
/// dropping them is what makes the peer retransmit packets we already hold — so
/// they go out regardless of what happened to their neighbours.
pub(crate) fn send_ack_packets<E>(
    acks: &[SequenceNumber],
    mut send_one: impl FnMut(&AnyMessage) -> Result<(), E>,
) -> Result<(), E> {
    let mut failure = None;
    for chunk in acks.chunks(MAX_ACKS_PER_PACKET) {
        let packets = chunk
            .iter()
            .map(|id| PacketAckPacketsBlock { id: id.get() })
            .collect();
        let message = AnyMessage::PacketAck(PacketAck { packets });
        if let Err(error) = send_one(&message)
            && failure.is_none()
        {
            failure = Some(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::{MAX_ACKS_PER_PACKET, send_ack_packets};
    use pretty_assertions::assert_eq;
    use sl_wire::SequenceNumber;
    use sl_wire::messages::AnyMessage;

    /// [`MAX_ACKS_PER_PACKET`] as the `u32` the tests count sequence numbers
    /// in; `the_chunk_bound_matches_the_module` pins the two together.
    const CHUNK: u32 = 255;

    /// The ids carried by a `PacketAck` message; empty for any other message,
    /// which fails the caller's comparison rather than the whole test binary.
    fn ids(message: &AnyMessage) -> Vec<u32> {
        match message {
            AnyMessage::PacketAck(ack) => ack.packets.iter().map(|p| p.id).collect(),
            _ => Vec::new(),
        }
    }

    /// `n` sequence numbers, `1..=n`, in the order they were owed.
    fn owed(n: u32) -> Vec<SequenceNumber> {
        (1..=n).map(SequenceNumber::new).collect()
    }

    #[test]
    fn the_chunk_bound_matches_the_module() {
        assert_eq!(usize::try_from(CHUNK), Ok(MAX_ACKS_PER_PACKET));
    }

    #[test]
    fn an_empty_batch_sends_nothing() {
        let mut sent: Vec<Vec<u32>> = Vec::new();
        let result: Result<(), ()> = send_ack_packets(&[], |message| {
            sent.push(ids(message));
            Ok(())
        });
        assert!(result.is_ok());
        assert!(sent.is_empty());
    }

    #[test]
    fn a_short_batch_is_one_packet_in_order() {
        let mut sent: Vec<Vec<u32>> = Vec::new();
        let result: Result<(), ()> = send_ack_packets(&owed(3), |message| {
            sent.push(ids(message));
            Ok(())
        });
        assert!(result.is_ok());
        assert_eq!(sent, vec![vec![1, 2, 3]]);
    }

    #[test]
    fn a_long_batch_splits_at_the_encodable_bound() {
        let count = CHUNK * 2 + 7;
        let mut sent: Vec<Vec<u32>> = Vec::new();
        let result: Result<(), ()> = send_ack_packets(&owed(count), |message| {
            sent.push(ids(message));
            Ok(())
        });
        assert!(result.is_ok());
        let lengths: Vec<usize> = sent.iter().map(Vec::len).collect();
        assert_eq!(
            lengths,
            vec![MAX_ACKS_PER_PACKET, MAX_ACKS_PER_PACKET, 7],
            "every chunk but the last is packed full"
        );
        let flat: Vec<u32> = sent.into_iter().flatten().collect();
        assert_eq!(flat, (1..=count).collect::<Vec<u32>>(), "no ack is dropped");
    }

    #[test]
    fn a_failed_packet_does_not_take_the_others_with_it() {
        let count = CHUNK * 3 + 1;
        let mut offered: Vec<Vec<u32>> = Vec::new();
        let mut attempt = 0_u32;
        let result = send_ack_packets(&owed(count), |message| {
            offered.push(ids(message));
            attempt += 1;
            if attempt == 2 { Err(attempt) } else { Ok(()) }
        });
        assert_eq!(result, Err(2), "the failure is reported, not swallowed");
        assert_eq!(offered.len(), 4, "every chunk is still offered");
        let flat: Vec<u32> = offered.into_iter().flatten().collect();
        assert_eq!(
            flat,
            (1..=count).collect::<Vec<u32>>(),
            "the acks behind the failure are not discarded"
        );
    }

    #[test]
    fn the_first_failure_is_the_one_reported() {
        let count = CHUNK * 3;
        let mut attempt = 0_u32;
        let result = send_ack_packets(&owed(count), |_message| {
            attempt += 1;
            if attempt == 1 { Ok(()) } else { Err(attempt) }
        });
        assert_eq!(result, Err(2));
    }
}
