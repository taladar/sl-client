//! Every asset stream the session has in flight, in one place.
//!
//! Eleven of [`Session`](crate::Session)'s fields were registries of pending
//! transfers — two id counters and nine stores that are all
//! insert-on-request, remove-on-completion, and each of which strands its
//! buffers (whole asset payloads, for an upload) and the caller waiting on it
//! if the answer never comes. They are [`Transfers`] now.
//!
//! The registries stay directly readable: each is a plain per-request store,
//! and the handler that drains one has nothing to say to the others. What the
//! type owns is what is true of *all* of them — the id allocation
//! ([`Transfers::mint_xfer_id`], [`Transfers::mint_transfer_id`]), the earliest
//! instant any of them needs a sweep ([`Transfers::next_deadline`]), and the
//! task-inventory claim expiry, whose two halves must be swept together.

use super::{
    ASSET_TRANSFER_TIMEOUT, OfferedUpload, PendingInventorySave, RELIABLE_REPLY_GRACE,
    TEXTURE_DOWNLOAD_STALL_TIMEOUT, TextureDownload, TransferDownload, XFER_STALL_TIMEOUT,
    XferDownload, XferUpload, deadline, merge_deadline,
};
use crate::bookkeeping_ids::{TransferId, XferId};
use sl_types::key::ObjectKey;
use std::collections::{BTreeMap, VecDeque};
use std::time::Instant;
use uuid::Uuid;

/// The session's in-flight asset streams: the downloads it is receiving, the
/// uploads it is sending, the offers waiting to be picked up, and the claims
/// waiting on a reply — plus the two id counters that address them.
#[derive(Debug)]
pub(crate) struct Transfers {
    /// In-flight inbound `Xfer` file downloads, keyed by the client-chosen
    /// [`XferId`], each carrying the accumulated bytes and a routing
    /// [`XferPurpose`](super::XferPurpose). Started by a `MuteListUpdate`, an
    /// auto-fetched `ReplyTaskInventory`, or
    /// [`Session::request_xfer`](crate::Session::request_xfer); the single
    /// `SendXferPacket` handler drains and routes them.
    pub(super) xfer_downloads: BTreeMap<XferId, XferDownload>,
    /// Files offered for outbound `Xfer` upload but not yet requested, keyed by
    /// the filename we named to the simulator. When the simulator answers a
    /// client upload trigger (today `EstateOwnerMessage`/`terrain`
    /// `["upload filename", …]`) with a `RequestXfer` naming this filename, the
    /// bytes move into [`xfer_uploads`](Self::xfer_uploads) under the
    /// simulator-assigned [`XferId`] and start streaming. Mirrors the reference
    /// viewer's `expectFileForTransfer` registry: we only upload a file the
    /// caller explicitly offered. An offer the simulator never picks up is
    /// withdrawn after [`XFER_OFFER_TIMEOUT`](super::XFER_OFFER_TIMEOUT).
    pub(super) pending_xfer_uploads: BTreeMap<String, OfferedUpload>,
    /// In-flight outbound `Xfer` file uploads, keyed by the **simulator-assigned**
    /// [`XferId`] from the `RequestXfer`. Each `ConfirmXferPacket` releases the
    /// next `SendXferPacket`; the final confirmation surfaces
    /// [`Event::XferUploaded`](crate::Event::XferUploaded).
    pub(super) xfer_uploads: BTreeMap<XferId, XferUpload>,
    /// Asset bytes offered for a legacy `AssetUploadRequest` upload that was too
    /// large to inline, keyed by the **predicted asset id** (`VFileID`). When the
    /// simulator answers with a `RequestXfer` whose `VFileID` matches, the bytes
    /// move into [`xfer_uploads`](Self::xfer_uploads) and stream. Mirrors the
    /// reference viewer's `LLAssetStorage::storeAssetData` Xfer fallback. An
    /// offer the simulator never picks up is withdrawn after
    /// [`XFER_OFFER_TIMEOUT`](super::XFER_OFFER_TIMEOUT), surfacing a failed
    /// [`Event::InventoryAssetSaved`](crate::Event::InventoryAssetSaved) so the
    /// save's caller is not left waiting.
    pub(super) pending_asset_uploads: BTreeMap<Uuid, OfferedUpload>,
    /// Legacy asset saves
    /// ([`Session::save_inventory_asset`](crate::Session::save_inventory_asset))
    /// awaiting their `AssetUploadComplete`, keyed by the **predicted asset id**
    /// the completion names — which is the only thing the wire completion
    /// carries, so this is what turns it back into the transaction the caller
    /// started.
    ///
    /// Every save is registered, inlined or not: the small ones (the common
    /// case) never reach [`pending_asset_uploads`](Self::pending_asset_uploads)
    /// at all, and they need correlating just as much. An entry the simulator
    /// never answers is withdrawn after
    /// [`INVENTORY_SAVE_TIMEOUT`](super::INVENTORY_SAVE_TIMEOUT), surfacing a
    /// failed [`Event::InventoryAssetSaved`](crate::Event::InventoryAssetSaved).
    pub(super) pending_inventory_saves: BTreeMap<Uuid, PendingInventorySave>,
    /// Objects whose task inventory a
    /// [`Session::fetch_task_inventory`](crate::Session::fetch_task_inventory)
    /// asked for, keyed by their full [`ObjectKey`] (resolved from the object
    /// cache at request time). When the matching `ReplyTaskInventory` arrives
    /// its `Xfer` listing is auto-downloaded and parsed into
    /// [`Event::TaskInventoryContents`](crate::Event::TaskInventoryContents)
    /// rather than surfaced only as a serial/filename. The value is when the
    /// request went out: a claim whose reply never arrives is dropped after
    /// [`RELIABLE_REPLY_GRACE`] rather than silently upgrading some later,
    /// unrelated `RequestTaskInventory` for the same object.
    pub(super) pending_task_inventory: BTreeMap<ObjectKey, Instant>,
    /// A FIFO fallback for `fetch_task_inventory` calls whose target object was
    /// not yet in the cache (so its full id could not be resolved to key
    /// [`pending_task_inventory`](Self::pending_task_inventory)). Each entry
    /// auto-fetches the next otherwise-unmatched `ReplyTaskInventory`; it cannot
    /// disambiguate concurrent uncached fetches. Each entry is the instant its
    /// request went out, and is dropped after [`RELIABLE_REPLY_GRACE`] — a claim
    /// left standing would otherwise hijack an unrelated later reply.
    pub(super) pending_task_inventory_unresolved: VecDeque<Instant>,
    /// In-flight legacy UDP texture downloads, keyed by the texture's asset id
    /// (echoed in every `ImageData`/`ImagePacket`). Started by
    /// [`Session::request_texture`](crate::Session::request_texture).
    pub(super) texture_downloads: BTreeMap<Uuid, TextureDownload>,
    /// In-flight legacy UDP asset Transfers (`TransferRequest` →
    /// `TransferInfo` + `TransferPacket` stream), keyed by the client-minted
    /// [`TransferId`]. Started by
    /// [`Session::fetch_task_item_asset`](crate::Session::fetch_task_item_asset)
    /// / [`Session::fetch_estate_covenant_asset`](crate::Session::fetch_estate_covenant_asset)
    /// — the two source types that remain UDP-only on both grids (no
    /// `ViewerAsset` coverage).
    pub(super) transfer_downloads: BTreeMap<TransferId, TransferDownload>,
    /// A monotonic counter for generating `Xfer` ids (never zero).
    next_xfer_id: XferId,
    /// A monotonic counter for minting [`TransferId`]s (never nil). The
    /// reference viewer mints random transfer ids; a sans-I/O session has no
    /// randomness, and the id only correlates replies on this circuit.
    next_transfer_id: u128,
}

impl Transfers {
    /// Nothing in flight. `const`, because
    /// [`Session::new`](crate::Session::new) is.
    pub(crate) const fn new() -> Self {
        Self {
            xfer_downloads: BTreeMap::new(),
            pending_xfer_uploads: BTreeMap::new(),
            xfer_uploads: BTreeMap::new(),
            pending_asset_uploads: BTreeMap::new(),
            pending_inventory_saves: BTreeMap::new(),
            pending_task_inventory: BTreeMap::new(),
            pending_task_inventory_unresolved: VecDeque::new(),
            texture_downloads: BTreeMap::new(),
            transfer_downloads: BTreeMap::new(),
            next_xfer_id: XferId(1),
            next_transfer_id: 1,
        }
    }

    /// The next client-chosen `Xfer` id, never zero.
    pub(crate) fn mint_xfer_id(&mut self) -> XferId {
        let id = self.next_xfer_id;
        self.next_xfer_id = XferId(self.next_xfer_id.get().checked_add(1).unwrap_or(1));
        id
    }

    /// The next client-minted [`TransferId`], never nil.
    pub(crate) fn mint_transfer_id(&mut self) -> TransferId {
        let id = TransferId::new(Uuid::from_u128(self.next_transfer_id));
        self.next_transfer_id = self.next_transfer_id.checked_add(1).unwrap_or(1);
        id
    }

    /// Drops the task-inventory claims whose `ReplyTaskInventory` never
    /// arrived, returning how many were lost (each is a caller still waiting).
    ///
    /// The unresolved queue is the one that matters: its entries are
    /// positional, so a claim left standing is filled by the next *unrelated*
    /// reply — an object whose contents nobody asked to read would be
    /// downloaded and parsed on a stale claim's behalf.
    pub(crate) fn expire_task_claims(&mut self, now: Instant) -> usize {
        let fresh = |asked: &Instant| now.saturating_duration_since(*asked) < RELIABLE_REPLY_GRACE;
        let unresolved_before = self.pending_task_inventory_unresolved.len();
        self.pending_task_inventory_unresolved.retain(fresh);
        let resolved_before = self.pending_task_inventory.len();
        self.pending_task_inventory.retain(|_, asked| fresh(asked));
        unresolved_before
            .saturating_sub(self.pending_task_inventory_unresolved.len())
            .saturating_add(resolved_before.saturating_sub(self.pending_task_inventory.len()))
    }

    /// The earliest instant at which any registry here has a sweep due, merged
    /// into [`Session::poll_timeout`](crate::Session::poll_timeout) so an
    /// otherwise idle shell still wakes for a stalled stream.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        let mut earliest = None;
        for download in self.texture_downloads.values() {
            merge_deadline(
                &mut earliest,
                Some(deadline(
                    download.last_progress,
                    TEXTURE_DOWNLOAD_STALL_TIMEOUT,
                )),
            );
        }
        for download in self.transfer_downloads.values() {
            merge_deadline(
                &mut earliest,
                Some(deadline(download.last_progress, ASSET_TRANSFER_TIMEOUT)),
            );
        }
        for download in self.xfer_downloads.values() {
            merge_deadline(
                &mut earliest,
                Some(deadline(download.last_progress, XFER_STALL_TIMEOUT)),
            );
        }
        for upload in self.xfer_uploads.values() {
            merge_deadline(
                &mut earliest,
                Some(deadline(upload.last_progress, XFER_STALL_TIMEOUT)),
            );
        }
        for offer in self.pending_xfer_uploads.values() {
            merge_deadline(&mut earliest, Some(offer.expires));
        }
        for offer in self.pending_asset_uploads.values() {
            merge_deadline(&mut earliest, Some(offer.expires));
        }
        for save in self.pending_inventory_saves.values() {
            merge_deadline(&mut earliest, Some(save.expires));
        }
        for asked in &self.pending_task_inventory_unresolved {
            merge_deadline(&mut earliest, Some(deadline(*asked, RELIABLE_REPLY_GRACE)));
        }
        for asked in self.pending_task_inventory.values() {
            merge_deadline(&mut earliest, Some(deadline(*asked, RELIABLE_REPLY_GRACE)));
        }
        earliest
    }
}
