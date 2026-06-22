//! The LLUDP packet sequence-number newtype.
//!
//! Every LLUDP datagram carries a four-byte big-endian *sequence number* in its
//! prelude (see [`crate::header`]). Outgoing packets number monotonically
//! (wrapping at `u32::MAX`); reliable packets are tracked by sequence until
//! acknowledged, and appended acknowledgements echo the sequence numbers of
//! reliable packets received.
//!
//! The raw `u32` is therefore three things the compiler can't otherwise tell
//! apart — an outgoing counter value, an unacked-set key, an inbound id to ack —
//! all distinct from any other 32-bit field. Wrapping it as a newtype (mirroring
//! [`RegionHandle`](crate::RegionHandle) and the `sl-types` key wrappers) keeps a
//! sequence number from being transposed with an unrelated `u32`, and gives the
//! wrapping increment a single named home.

/// A four-byte LLUDP packet sequence number (the reference viewer's
/// `LLPacketBuffer` packet id / `mSequenceNumber`).
///
/// Sequence numbers count up from `1` and wrap at `u32::MAX`. The same type
/// names an outgoing packet's id, a key in the unacknowledged-packet set, and an
/// inbound id owed an acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct SequenceNumber(pub u32);

impl SequenceNumber {
    /// The first sequence number a circuit allocates.
    pub const FIRST: Self = Self(1);

    /// Builds a sequence number from its raw `u32` wire value.
    #[must_use]
    pub const fn new(sequence: u32) -> Self {
        Self(sequence)
    }

    /// Returns the raw `u32` wire value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Returns the next sequence number, wrapping at `u32::MAX` back to `0`
    /// (matching the viewer's free-running packet counter).
    #[must_use]
    pub const fn wrapping_next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

impl core::fmt::Display for SequenceNumber {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::SequenceNumber;
    use pretty_assertions::assert_eq;

    #[test]
    fn round_trips_raw_value() {
        let seq = SequenceNumber::new(42);
        assert_eq!(seq.get(), 42);
        assert_eq!(SequenceNumber(seq.get()), seq);
        assert_eq!(seq.to_string(), "42");
    }

    #[test]
    fn first_is_one() {
        assert_eq!(SequenceNumber::FIRST, SequenceNumber(1));
    }

    #[test]
    fn wrapping_next_advances_and_wraps() {
        assert_eq!(SequenceNumber(1).wrapping_next(), SequenceNumber(2));
        // Wraps at u32::MAX back to 0, matching the free-running counter.
        assert_eq!(SequenceNumber(u32::MAX).wrapping_next(), SequenceNumber(0));
    }

    #[test]
    fn orders_by_raw_value() {
        // Ord is needed because sequence numbers key the unacked BTreeMap.
        assert!(SequenceNumber(1) < SequenceNumber(2));
    }
}
