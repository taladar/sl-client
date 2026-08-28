//! Reading the outstanding reliable packets as a window on a wrapping counter.
//!
//! Outgoing sequence numbers are a free-running `u32` that wraps at
//! `u32::MAX`, so the unacknowledged packets of a long-lived circuit are not an
//! interval of the integers — they are an interval of the *circle*, and once
//! the counter has wrapped the two halves sit at opposite ends of the numeric
//! range. A [`BTreeMap`] keyed by [`SequenceNumber`] orders them numerically,
//! which is the same order only while the circuit has not wrapped.
//!
//! Both directions of the protocol keep that map and both have to name its
//! oldest entry for the `OldestUnacked` field of a keep-alive `StartPingCheck`
//! ([`Circuit`](crate::session::Circuit) and [`SimSession`](crate::SimSession)),
//! so the rule for reading the circle out of the map lives here, once.

use sl_wire::SequenceNumber;
use std::collections::BTreeMap;
use std::ops::Bound;

/// The oldest unacknowledged outgoing sequence number, given the set of
/// outstanding packets and the counter `next_sequence` the circuit will
/// allocate from next.
///
/// The counter is the seam of the circle: everything still unacknowledged was
/// sent before it, so a key *above* `next_sequence` is one the counter has
/// already passed and wrapped away from — older than every key below it, which
/// was sent since the wrap. The oldest entry is therefore the first key
/// strictly greater than the counter, and only when there is none (the usual,
/// un-wrapped case) the numerically smallest one. This is the reference
/// viewer's split in `LLCircuitData::pingTimerExpired`, whose
/// `mUnackedPackets.upper_bound(getPacketOutID())` names the same key.
///
/// With nothing outstanding the counter itself is reported, again as the
/// reference viewer does: it is one past every sequence number the peer could
/// still be holding a duplicate-suppression record for, so it retires all of
/// them.
pub(crate) fn oldest<T>(
    unacked: &BTreeMap<SequenceNumber, T>,
    next_sequence: SequenceNumber,
) -> SequenceNumber {
    unacked
        .range((Bound::Excluded(next_sequence), Bound::Unbounded))
        .next()
        .or_else(|| unacked.first_key_value())
        .map_or(next_sequence, |(sequence, _)| *sequence)
}

#[cfg(test)]
mod tests {
    use super::oldest;
    use pretty_assertions::assert_eq;
    use sl_wire::SequenceNumber;
    use std::collections::BTreeMap;

    /// An unacked set holding the given sequence numbers. Only the keys take
    /// part in the ordering, so each packet stands in as its own raw sequence.
    fn unacked(sequences: &[u32]) -> BTreeMap<SequenceNumber, u32> {
        sequences
            .iter()
            .map(|sequence| (SequenceNumber::new(*sequence), *sequence))
            .collect()
    }

    #[test]
    fn nothing_outstanding_reports_the_counter() {
        // One past everything the peer could still be suppressing, so the ping
        // retires its whole record rather than none of it.
        let empty = unacked(&[]);
        assert_eq!(oldest(&empty, SequenceNumber::new(42)), SequenceNumber(42));
    }

    #[test]
    fn before_a_wrap_the_oldest_is_the_lowest() {
        let outstanding = unacked(&[5, 7, 9]);
        assert_eq!(
            oldest(&outstanding, SequenceNumber::new(10)),
            SequenceNumber(5)
        );
    }

    #[test]
    fn across_a_wrap_the_oldest_is_the_first_above_the_counter() {
        // The counter has wrapped past `u32::MAX` and reached 3; 5 and 7 were
        // sent before the wrap and 0 and 1 after it, so 5 is the oldest even
        // though it is the numerically largest key.
        let outstanding = unacked(&[0, 1, 5, 7]);
        assert_eq!(
            oldest(&outstanding, SequenceNumber::new(3)),
            SequenceNumber(5)
        );
    }

    #[test]
    fn a_wrap_at_the_end_of_the_range_is_no_different() {
        // The realistic shape of a wrap: a contiguous run straddling `u32::MAX`.
        let outstanding = unacked(&[u32::MAX - 1, u32::MAX, 0, 1]);
        assert_eq!(
            oldest(&outstanding, SequenceNumber::new(2)),
            SequenceNumber(u32::MAX - 1)
        );
    }

    #[test]
    fn everything_outstanding_may_sit_above_the_counter() {
        // Nothing has been sent since the wrap, so there is no key below the
        // counter to be tempted by.
        let outstanding = unacked(&[u32::MAX - 2, u32::MAX]);
        assert_eq!(
            oldest(&outstanding, SequenceNumber::new(0)),
            SequenceNumber(u32::MAX - 2)
        );
    }

    #[test]
    fn a_single_outstanding_packet_is_the_oldest_either_way() {
        let before = unacked(&[9]);
        assert_eq!(oldest(&before, SequenceNumber::new(10)), SequenceNumber(9));
        let after = unacked(&[u32::MAX]);
        assert_eq!(
            oldest(&after, SequenceNumber::new(1)),
            SequenceNumber(u32::MAX)
        );
    }
}
