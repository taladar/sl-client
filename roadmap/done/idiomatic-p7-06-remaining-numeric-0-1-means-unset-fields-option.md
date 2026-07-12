---
id: idiomatic-p7-06
title: Remaining numeric 0/-1-means-unset fields** → Option:
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 7 — second-pass audit (missed ids, in-band sentinels, non-masking)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

**Remaining numeric `0`/`-1`-means-unset fields** → `Option`:
`InstantMessage.timestamp`/`Event::ConferenceInvited.timestamp` (0 = unset),
`ParcelMediaUpdateInfo` `media_width`/`media_height` (0 = native), and the
`InventoryCallbackId` `0`-no-callback call site. **DONE** (2026-06-24). Added
two reusable codec helpers in `sl-proto/src/types.rs`
(`optional_u32_from_wire`/`optional_u32_to_wire` for the `u32` timestamp
fields, `optional_i32_from_wire` for the decode-only `i32` media dimensions —
no `_to_wire` since `ParcelMediaUpdateInfo` is never encoded client-side),
mirroring the existing `optional_uuid_*` boundary helpers. `0` ⇄ `None` is
wire byte-identical. Converted: `InstantMessage.timestamp` → `Option<u32>`
(UDP `instant_message` decode + the offline-IM LLSD decode/encode in
`conversions.rs`); `Event::ConferenceInvited.timestamp` → `Option<u32>` (the
`ChatterBoxInvitation` CAPS decode/encode); `ParcelMediaUpdateInfo`
`media_width`/`media_height` → `Option<i32>` (the `ParcelMediaUpdate` decode);
`Event::InventoryItemCreated.callback_id` → `Option<InventoryCallbackId>`
(the `UpdateCreateInventoryItem` decode — `0` = an item the sim materialised
with no client request, e.g. an accepted inventory offer). The
`InventoryBulkUpdate.item_callbacks` site already filtered `callback_id != 0`,
so it needed no change. `next_inventory_callback` and the
`create`/`copy_inventory_item` returns stay non-`Option` (they always allocate
a real id). REPL/survey don't read these fields (they match the variant with
`..`); the one example reading `media_width`/`media_height`
(`tokio_login_hold_logout`) uses `.unwrap_or(0)`. +1 unit test
(`optional_numeric_wire_maps_zero_to_none`, incl. a negative `i32` is *not* a
sentinel); lifecycle + `sim_session` suites updated. NO sl-types touched.
Build + clippy (--workspace --all-targets) + all tests green. **Phase 7 B
COMPLETE.**

- **Exceptions (kept in-band — sentinel is in the value domain):** open enums
preserving `Unknown(raw)`; the polymorphic `MapItem.name`; outbound search
filters (`DirPlacesQuery.sim_name`) that are partial query strings, not
identities.

**C — non-masking decode (always hard error; IN PROGRESS):**
