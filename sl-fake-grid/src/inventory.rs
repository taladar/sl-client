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
//! # An upload is announced by a different rule ([`UploadAnnouncements`])
//!
//! [`InventoryAnnouncement`] is what a **take** reads, and it would be a
//! reasonable guess that an upload reads it too. It does not, and the two grids
//! are the reason: measured (2026-09-08) they take *opposite* sides here from
//! the ones they take on a take — and they only disagree about one of the two
//! upload paths.
//!
//! | after a CAPS upload completes | Second Life | OpenSim |
//! | --- | --- | --- |
//! | in-place asset save (`Update*AgentInventory`) | the legacy UDP `UpdateCreateInventoryItem` | nothing at all |
//! | a `NewFileAgentInventory` completion | nothing at all | nothing at all |
//!
//! So on a take Second Life is the grid that pushes a `BulkUpdateInventory` and
//! OpenSim the one that sends the legacy message, while after a save it is
//! Second Life that sends the legacy message and OpenSim that sends nothing.
//! One enum could not have described both, which is why the uploads have
//! [`UploadAnnouncement`] of their own — and one *value* could not describe
//! both rows of the table above, which is why a grid carries an
//! [`UploadAnnouncements`] pair rather than a single answer.
//!
//! Where each number comes from: the conformance case `notecard-create-update`
//! records `save_announcement` on both grids (aditi:
//! `update-create-inventory-item`, OpenSim: `none`) and `asset-upload` records
//! `upload_announcement` on both (`none` either side). The reference viewer
//! needs neither: `LLBufferedAssetUploadInfo::finishUpload` builds the item
//! from the HTTP response body and calls `gInventory.notifyObservers()`.
//!
//! # The second row was extrapolated the wrong way, and the measurement said so
//!
//! Worth keeping, because the reasoning that produced the wrong answer is not
//! obviously bad and someone will produce it again. When only the first row was
//! affordable to measure — Second Life's `NewFileAgentInventory` takes only the
//! chargeable upload classes, so reaching that completion costs an upload fee —
//! the second row was filled in from the first: same grid, same capability
//! family, and the message in question is the general-purpose legacy "here is
//! an item you now have", so a grid that kept the push for a *rewritten* item
//! and dropped it for a *created* one looked like the odd one.
//!
//! It is the odd one, and it is what Second Life does. Paying the fee (aditi,
//! 2026-09-08, a 64×64 texture at the account's own L$ 10 benefits price)
//! recorded `upload_announcement = none` on two runs. The distinction the
//! extrapolation missed is **what the client already knows**: a
//! `NewFileAgentInventory` response body carries the whole new item, so a push
//! would tell the viewer nothing it is not holding, while an in-place save's
//! response names only the new *asset* — the item's own copy in the viewer's
//! model still points at the asset that was replaced, and the legacy push is
//! what repoints it. Read that way the two rows stop being a contradiction: the
//! push survives exactly where it still carries information.
//!
//! # The take and the save diverge for opposite reasons
//!
//! Reading the two disagreements as "the grids simply disagree twice" would get
//! the save backwards. **The push is the older behaviour and OpenSim is the
//! grid that omits it** — it is not something Second Life added.
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
//! So the take is Second Life having **moved on** — inventory went behind AIS3
//! and the take's announcement went with it — and the save is OpenSim having
//! **never sent** what a Linden simulator sends. That is also why the save's
//! Second Life side is the legacy message rather than a modern one: it is the
//! same `UpdateCreateInventoryItem` that has always announced a created or
//! rewritten item.
//!
//! Which means the two grids' agreement on the creation path is a **coincidence
//! of two different omissions**, not a shared rule: OpenSim is quiet there
//! because it is quiet after every capability upload, and Second Life because
//! that one response already carries the item. So the pair is two values and
//! not one shared answer, and an OpenSim that started announcing would not drag
//! the other flavour with it.
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

/// How the simulator tells a client about an item **one** capability upload
/// path just bound — a `NewFileAgentInventory` completion, or an asset saved in
/// place over one of the `Update*AgentInventory` capabilities.
///
/// Separate from [`InventoryAnnouncement`] because the two grids swap sides
/// between a take and an upload; a pair of these ([`UploadAnnouncements`])
/// rather than one because the two upload paths are answered differently on the
/// same grid. See the module docs for the measurements.
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
    /// upload path, and what Second Life does after a `NewFileAgentInventory`
    /// completion (aditi, 2026-09-08).
    ///
    /// An omission on the *grid's* part, but not one on a client's: the
    /// reference viewer builds the item out of the response body and never
    /// wanted the push. A viewer that instead waits for one hangs against a
    /// grid set this way, which is the failure this side exists to reproduce.
    Silent,
}

/// How a grid answers **both** capability upload paths, which is two answers
/// and not one.
///
/// The paths are not interchangeable and neither live grid treats them as such:
/// see the module docs for the measured table, and for why the push survives
/// exactly where it still tells the client something the HTTP response did not.
///
/// The [default](Self::default) is the Second Life pair, matching the default
/// flavour: silent after a creation, the legacy push after a save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadAnnouncements {
    /// What follows a `NewFileAgentInventory` completion, which **created** the
    /// item it names.
    ///
    /// [`UploadAnnouncement::Silent`] on both live grids: the completion's own
    /// response body carries the whole item, so a push would repeat what the
    /// client is already holding.
    pub created: UploadAnnouncement,
    /// What follows an in-place `Update*AgentInventory` save, which **rewrote**
    /// an item that already existed.
    ///
    /// The divergent one — the legacy UDP push on Second Life, nothing on
    /// OpenSim — and the one that still carries information: the response names
    /// the new asset, not the item, so a client that does not hear this keeps an
    /// item pointing at the asset the save replaced.
    pub saved: UploadAnnouncement,
}

impl Default for UploadAnnouncements {
    /// The Second Life pair, matching the default flavour — and **not** the
    /// field-wise default of [`UploadAnnouncement`], whose own default is the
    /// legacy push because that is what Second Life sends on the one path that
    /// still sends anything.
    fn default() -> Self {
        Self {
            created: UploadAnnouncement::Silent,
            saved: UploadAnnouncement::Legacy,
        }
    }
}

impl UploadAnnouncements {
    /// Both paths answered the same way — the shape a grid takes when it
    /// announces every capability upload, or none of them.
    ///
    /// Neither live grid is one of these; it is here for a test that wants to
    /// hold one path still while it varies the other, and for the
    /// [`FakeGridBuilder`](crate::FakeGridBuilder) override to be expressible in
    /// one call.
    #[must_use]
    pub const fn uniform(announcement: UploadAnnouncement) -> Self {
        Self {
            created: announcement,
            saved: announcement,
        }
    }
}
