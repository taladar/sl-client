//! Pipeline status snapshots for store instrumentation.
//!
//! A diagnostics HUD (and any other observer) needs a cheap, point-in-time view
//! of how much work an asset store is doing and how full its admission gate is.
//! These snapshots are deliberately domain-free — a [`StoreStats`] buckets an
//! arbitrary store's live entries by the stage they are in (queued, reading
//! disk, downloading, decoding, ready, failed, cancelled), and adds the
//! in-memory entry count, an approximate in-memory byte footprint, and the
//! cumulative disk-cache-hit and garbage-collected counters. Each higher-level
//! store maps its own progress enum onto these buckets. [`GateStats`] mirrors a
//! [`PriorityGate`](crate::PriorityGate)'s capacity, in-flight, and waiting
//! figures.

/// A point-in-time snapshot of an asset store's fetch/decode pipeline.
///
/// The by-stage counts (`queued` … `cancelled`) partition the store's live
/// (still-referenced) entries — their sum is [`in_memory`](Self::in_memory).
/// `cache_hits` and `collected` are cumulative counters since the store was
/// created; `bytes` is the current approximate in-memory footprint.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `StoreStats` reads clearly"
)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StoreStats {
    /// Entries registered and awaiting a work slot behind the admission gate.
    pub queued: usize,
    /// Entries reading their raw bytes from the on-disk cache.
    pub reading_disk: usize,
    /// Entries downloading their bytes over HTTP.
    pub downloading: usize,
    /// Entries decoding fetched bytes to their in-memory representation.
    pub decoding: usize,
    /// Entries decoded/fetched and available.
    pub ready: usize,
    /// Entries whose fetch or decode failed.
    pub failed: usize,
    /// Entries cancelled before completion (the last requester withdrew).
    pub cancelled: usize,
    /// Live entries currently held in memory (the weak map's upgradeable slots).
    pub in_memory: usize,
    /// Approximate bytes held in memory across all live entries.
    pub bytes: u64,
    /// Cumulative disk-cache hits (bytes served from disk) since creation.
    pub cache_hits: u64,
    /// Cumulative entries garbage-collected (swept) since creation.
    pub collected: u64,
}

/// A point-in-time snapshot of a [`PriorityGate`](crate::PriorityGate).
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `GateStats` reads clearly"
)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct GateStats {
    /// Total concurrent work slots the gate admits.
    pub capacity: usize,
    /// Slots currently held (permits acquired but not yet released).
    pub in_flight: usize,
    /// Requests queued behind the gate, waiting for a slot.
    pub waiting: usize,
}
