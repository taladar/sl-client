//! How this grid does inventory: whether the deprecated UDP fetch still works,
//! and how a newly created item is announced.
//!
//! The two are one subject because they are the same divergence seen from
//! either end. OpenSim's inventory is still the UDP one — a viewer may fetch a
//! folder with `FetchInventoryDescendents` and is told about a new item with
//! `UpdateCreateInventoryItem`. Second Life moved inventory to AIS3: the UDP
//! fetch is gone, and the new item arrives as a `BulkUpdateInventory` over the
//! event queue. A viewer that quietly depends on either legacy half works
//! against one live grid and not the other, which is exactly what this grid
//! exists to make visible, so both follow
//! [`ImitatedGrid`](crate::ImitatedGrid).
//!
//! # The announcement's *order* changes with it too
//!
//! Worth stating because it caught a test that had assumed otherwise. A take
//! sends the filed item and the world's `KillObject`s in the same breath, but
//! the two travel differently: the kills go out over UDP straight away, while a
//! Second-Life-flavoured announcement rides the event queue and reaches the
//! client on its next long-poll. So on that flavour **the item arrives after
//! the kills**, and a consumer that waits for the item before looking for the
//! kills has already discarded them.
//!
//! # What [`InventoryAnnouncement`] does not govern yet
//!
//! A **take** reads it. The upload paths (`uploads.rs`) do not: a
//! `NewFileAgentInventory` completion, an in-place asset save and the legacy
//! `UpdateInventoryItem` transaction all still hand the item over with the
//! legacy UDP message on both flavours. That is stated rather than fixed
//! because it is unmeasured: on Second Life each of those is the reply to a
//! *capability* the client called, and the HTTP response already carries the
//! new item id and asset id — the reference viewer builds the item from the
//! response body rather than waiting for a push — so whether a real simulator
//! also announces it, and with which message, is not something this workspace
//! has looked at. Deriving it from the flavour would be inventing a behaviour,
//! which is the one thing [`ImitatedGrid`](crate::ImitatedGrid) is not for.
//! See `test-fake-grid-imitates-upload-announcements`.

/// How the simulator answers the deprecated UDP inventory fetch
/// (`FetchInventoryDescendents`).
///
/// Three roads, because the live grids take two of them and the third is what
/// makes a grid that does *not* serve the fetch say so out loud.
#[expect(
    clippy::module_name_repetitions,
    reason = "the name is read at its use sites -- `sl_fake_grid::LegacyUdpInventory`, \
              `FakeGridBuilder::legacy_udp_inventory(LegacyUdpInventory::Served)` -- \
              where a bare `LegacyUdp` would not say what path is meant"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LegacyUdpInventory {
    /// Answer it with a `FeatureDisabled` naming the refused feature — what
    /// Second Life does for a message it has blacklisted, and what the
    /// reference viewer logs as its "Blacklisted Feature Response".
    ///
    /// The default, and what a Second-Life-flavoured grid does: of the two
    /// roads a grid without the UDP path has, only this one produces something
    /// to assert, because silence is indistinguishable from a lost packet.
    #[default]
    Refused,
    /// Drop it silently — what Second Life empirically does to this particular
    /// deprecated fetch (aditi, 2026-08-12).
    ///
    /// Faithful to the measurement, and available for a test that wants a
    /// viewer to meet the real thing; not the flavour's default, because a case
    /// asserting it can only wait out its own timeout.
    Ignored,
    /// Serve it: answer with `InventoryDescendents` out of the session's
    /// serving tree
    /// ([`SimSession::send_inventory_descendents`](sl_proto::SimSession::send_inventory_descendents)),
    /// which is what OpenSim still does.
    Served,
}

/// How the simulator tells a client about an inventory item it just created —
/// the item a take files away, chiefly.
#[expect(
    clippy::module_name_repetitions,
    reason = "the name is read at its use sites -- `sl_fake_grid::InventoryAnnouncement`, \
              `FakeGridBuilder::inventory_announcement(..)` -- where a bare \
              `Announcement` would say nothing about what is being announced"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InventoryAnnouncement {
    /// The legacy UDP `UpdateCreateInventoryItem`
    /// ([`SimSession::send_inventory_item_created`](sl_proto::SimSession::send_inventory_item_created)),
    /// which is what OpenSim sends.
    Legacy,
    /// A `BulkUpdateInventory` over the CAPS event queue
    /// ([`SimSession::enqueue_bulk_update_inventory`](sl_proto::SimSession::enqueue_bulk_update_inventory)),
    /// which is what Second Life sends now that inventory lives behind AIS3.
    ///
    /// The default, because the default flavour is Second Life.
    #[default]
    BulkUpdate,
}
