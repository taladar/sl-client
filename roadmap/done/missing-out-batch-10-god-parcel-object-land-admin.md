---
id: missing-out-batch-10
title: god parcel/object/land admin
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

**Out batch 10 — god parcel/object/land admin.** `ParcelGodForceOwner`,
`ParcelGodMarkAsContent`, `EventGodDelete`, `StateSave` (god region state
save), `ViewerStartAuction`. All NotTrusted, viewer-sent with the god bit
set; gated on the agent holding grid-god rights. New `Session` methods
`parcel_god_force_owner(parcel: ScopedParcelId, owner: OwnerKey)` (force-
reassign parcel ownership), `parcel_god_mark_as_content(parcel:
ScopedParcelId)` (mark a parcel and its content as governor-owned),
`event_god_delete(event: EventId, query_id: QueryId, query_text, flags:
DirFindFlags, query_start)` (delete an events-directory listing and re-run
the search so the simulator returns the refreshed page, mirroring
`dir_find_query`), `state_save(filename)` (save the region/world state; an
empty filename lets the simulator pick the autosave name, as the reference
viewer does), and `viewer_start_auction(parcel: ScopedParcelId, snapshot:
Option<TextureKey>)` (start a land auction). No new typed wrappers were
needed — the payloads reuse the existing typed `ScopedParcelId` /
`OwnerKey` / `EventId` / `QueryId` / `DirFindFlags` / `TextureKey`. Wired as
`Command::{ParcelGodForceOwner, ParcelGodMarkAsContent, EventGodDelete,
StateSave, ViewerStartAuction}` through the tokio and bevy runtimes, the
`command_name` formatter, and the `parcel_god_force_owner` /
`parcel_god_mark_as_content` / `event_god_delete` / `state_save` /
`viewer_start_auction` REPL tokens. Covered by one pack-the-wire lifecycle
test and five REPL parse tests; all are SL-/god-only so the round-trips
exercise against aditi (OpenSim does not honour the god bit without estate
config).
