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
//! # An upload is announced by a different rule ([`UploadAnnouncement`])
//!
//! [`InventoryAnnouncement`] is what a **take** reads, and it would be a
//! reasonable guess that an upload reads it too. It does not, and the two grids
//! are the reason: measured (2026-09-08) they take *opposite* sides here from
//! the ones they take on a take.
//!
//! | after a CAPS upload completes | Second Life | OpenSim |
//! | --- | --- | --- |
//! | in-place asset save (`Update*AgentInventory`) | the legacy UDP `UpdateCreateInventoryItem` | nothing at all |
//! | a `NewFileAgentInventory` completion | not measurable with a free asset class | nothing at all |
//!
//! So on a take Second Life is the grid that pushes a `BulkUpdateInventory` and
//! OpenSim the one that sends the legacy message, while after a save it is
//! Second Life that sends the legacy message and OpenSim that sends nothing.
//! One enum could not have described both, which is why the uploads have
//! [`UploadAnnouncement`] of their own.
//!
//! Where each number comes from: the conformance case `notecard-create-update`
//! records `save_announcement` on both grids (aditi:
//! `update-create-inventory-item`, OpenSim: `none`) and `asset-upload` records
//! `upload_announcement` on OpenSim (`none`). The reference viewer needs
//! neither: `LLBufferedAssetUploadInfo::finishUpload` builds the item from the
//! HTTP response body and calls `gInventory.notifyObservers()`.
//!
//! # The two rows diverge for opposite reasons
//!
//! Reading the table as "the grids simply disagree twice" would get the second
//! row backwards. **The push is the older behaviour and OpenSim is the grid
//! that omits it** — it is not something Second Life added.
//!
//! OpenSim's own source says so. At the in-place save,
//! `InventoryAccessModule.CapsUpdateInventoryItemAsset` ends on a
//! **commented-out** `// remoteClient.SendInventoryItemCreateUpdate(item);` and
//! answers with an `AlertMessage` instead — and that line has been commented
//! out since 2007-08, when the capability path was first written (it was
//! carried through the 2007-12 rename from `SendInventoryItemUpdate` and the
//! 2010 move into `InventoryAccessModule` still commented). At the
//! `NewFileAgentInventory` completion, `Scene.AddUploadedInventoryItem` reaches
//! inventory through the *client-less* `AddInventoryItem` overload, right
//! beside the one that does announce. Both sites had the announcing call
//! available and neither uses it.
//!
//! So the first row is Second Life having **moved on** — inventory went behind
//! AIS3 and the take's announcement went with it — and the second is OpenSim
//! having **never sent** what a Linden simulator sends. That is also why the
//! second row's Second Life side is the legacy message rather than a modern
//! one: it is the same `UpdateCreateInventoryItem` that has always announced a
//! created or rewritten item.
//!
//! **The one half that is extrapolated rather than measured** is Second Life's
//! `NewFileAgentInventory` completion, and it cannot be measured the way the
//! others were: that capability accepts only the chargeable file-upload classes
//! on Second Life (it answers a notecard with `Invalid asset type`, which is
//! why `asset-upload` records `partial` there), so reaching it needs an upload
//! fee and the price list `test-fake-grid-imitates-economy` has yet to measure.
//! Until then the fake grid announces a Second-Life-flavoured upload the same
//! way whichever of the two paths minted the item. That is a weaker claim than
//! it looks: the message being extrapolated is the general-purpose legacy
//! "here is an item you now have", the grid was measured sending exactly it for
//! the neighbouring path, and the alternative — Second Life keeping the legacy
//! push for a *rewritten* item but dropping it for a *created* one — would be
//! the odd behaviour needing evidence. It still wants measuring; see
//! `test-fake-grid-imitates-sl-new-file-upload-announcement`.
//!
//! # What follows neither: the legacy UDP transaction
//!
//! `UpdateInventoryItem` — the second half of a wearable save, which has no
//! capability — is answered with the legacy `UpdateCreateInventoryItem` on both
//! flavours, and that is not a gap. It is the **reply to a UDP request**, not a
//! push after an HTTP one: it echoes the transaction and callback ids the
//! client sent, and a client's save (`Command::SaveInventoryAsset` →
//! `Event::InventoryAssetSaved`) has nothing else to complete on. OpenSim sends
//! it there (`AssetXferUploader.SendInventoryItem`) precisely where it sends
//! nothing after a capability upload.

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

/// How the simulator tells a client about an item a **capability upload** just
/// created or rewrote — a `NewFileAgentInventory` completion, or an asset saved
/// in place over one of the `Update*AgentInventory` capabilities.
///
/// Separate from [`InventoryAnnouncement`] because the two grids swap sides
/// between the two questions: see the module docs for the measurements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UploadAnnouncement {
    /// Push the item over the legacy UDP `UpdateCreateInventoryItem`
    /// ([`SimSession::send_inventory_item_created`](sl_proto::SimSession::send_inventory_item_created)),
    /// which is what Second Life was measured doing after an in-place save
    /// (aditi, 2026-09-08).
    ///
    /// The default, because the default flavour is Second Life.
    #[default]
    Legacy,
    /// Say nothing: the capability's HTTP response named the asset and the item,
    /// and that is the whole of the answer — what OpenSim does after either
    /// upload path.
    ///
    /// An omission on the *grid's* part, but not one on a client's: the
    /// reference viewer builds the item out of the response body and never
    /// wanted the push. A viewer that instead waits for one hangs against a
    /// grid set this way, which is the failure this side exists to reproduce.
    Silent,
}
