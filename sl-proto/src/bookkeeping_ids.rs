//! Session bookkeeping id newtypes — the transient correlation ids the session
//! mints (or echoes) to match a reply to the request that provoked it.
//!
//! Unlike persistent UUID keys, these ids only matter for the lifetime of a
//! single exchange: a [`PingId`] correlates a `CompletePingCheck` with its
//! `StartPingCheck`, a [`TransferId`]/[`XferId`] correlates the packets of one
//! asset transfer or file transfer, and an [`InventoryCallbackId`] lets a caller
//! match an inventory reply to the request it issued. The raw integers (and the
//! transfer `Uuid`) carry that correlation role the compiler can't otherwise
//! see, so they live here as newtypes — mirroring
//! [`RegionHandle`](sl_wire::RegionHandle) and the `sl-types` key wrappers — and
//! can't be transposed with one another or with any other integer field.

use uuid::Uuid;

/// A `StartPingCheck`/`CompletePingCheck` ping id (the reference viewer's
/// `LLCircuitData::mLastPingID`).
///
/// A circuit numbers its outgoing pings with a wrapping `u8`; the matching
/// `CompletePingCheck` echoes the id so a round trip can be paired up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct PingId(pub u8);

impl PingId {
    /// Builds a ping id from its raw `u8` wire value.
    #[must_use]
    pub const fn new(id: u8) -> Self {
        Self(id)
    }

    /// Returns the raw `u8` wire value.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Returns the next ping id, wrapping at `u8::MAX`.
    #[must_use]
    pub const fn wrapping_next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

impl core::fmt::Display for PingId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A legacy file-transfer (`Xfer`) id — the `u64` the simulator (for an upload)
/// or the client (for a download) assigns to one `RequestXfer` →
/// `SendXferPacket`/`ConfirmXferPacket` exchange (the reference viewer's
/// `LLXfer::mID`).
///
/// It correlates every packet of a single chunked file transfer (a mute-list
/// download, an inventory-asset upload, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct XferId(pub u64);

impl XferId {
    /// Builds an xfer id from its raw `u64` wire value.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the raw `u64` wire value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl core::fmt::Display for XferId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An asset-transfer id — the `LLUUID` correlating the `TransferInfo` and
/// `TransferPacket`s of one `TransferRequest` (the reference viewer's
/// `LLTransferManager` transfer id).
///
/// Although it is carried on the wire as a UUID, it is *not* a persistent asset
/// key: the client mints a fresh one per `TransferRequest` purely to demultiplex
/// the inbound packets of concurrent transfers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct TransferId(pub Uuid);

impl TransferId {
    /// Builds a transfer id from its raw `Uuid` wire value.
    #[must_use]
    pub const fn new(id: Uuid) -> Self {
        Self(id)
    }

    /// Builds a transfer id from a `u128`, as the client does when minting one
    /// from its monotonic counter.
    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(Uuid::from_u128(value))
    }

    /// Returns the raw `Uuid` wire value.
    #[must_use]
    pub const fn get(self) -> Uuid {
        self.0
    }
}

impl core::fmt::Display for TransferId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An inventory async-callback id — the `u32` `CallbackID` a client allocates on
/// an inventory request (`CreateInventoryItem`, `CopyInventoryItem`, …) and the
/// simulator echoes in the resulting `UpdateCreateInventoryItem` /
/// `BulkUpdateInventory` so the reply can be matched to its request.
///
/// `0` is the conventional "no callback" sentinel (the simulator does not echo
/// a correlation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct InventoryCallbackId(pub u32);

impl InventoryCallbackId {
    /// Builds an inventory callback id from its raw `u32` wire value.
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Returns the raw `u32` wire value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl core::fmt::Display for InventoryCallbackId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{InventoryCallbackId, PingId, TransferId, XferId};
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    #[test]
    fn ping_id_round_trips_and_wraps() {
        let id = PingId::new(7);
        assert_eq!(id.get(), 7);
        assert_eq!(id.to_string(), "7");
        assert_eq!(PingId(254).wrapping_next(), PingId(255));
        assert_eq!(PingId(255).wrapping_next(), PingId(0));
    }

    #[test]
    fn xfer_id_round_trips_raw_value() {
        let id = XferId::new(0x0102_0304_0506_0708);
        assert_eq!(id.get(), 0x0102_0304_0506_0708);
        assert_eq!(XferId(id.get()), id);
    }

    #[test]
    fn transfer_id_round_trips_via_uuid_and_u128() {
        let uuid = Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888);
        let id = TransferId::new(uuid);
        assert_eq!(id.get(), uuid);
        assert_eq!(TransferId::from_u128(1).get(), Uuid::from_u128(1));
        assert_eq!(id.to_string(), uuid.to_string());
    }

    #[test]
    fn inventory_callback_id_round_trips_raw_value() {
        let id = InventoryCallbackId::new(42);
        assert_eq!(id.get(), 42);
        assert_eq!(id.to_string(), "42");
        assert_eq!(InventoryCallbackId::default(), InventoryCallbackId(0));
    }
}
