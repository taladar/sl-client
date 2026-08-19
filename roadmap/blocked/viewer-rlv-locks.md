---
id: viewer-rlv-locks
title: RLV — attachment, wearable and folder locks
topic: viewer
status: blocked
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-rlv-enforce-send-side, viewer-rlv-enforce-forced-actions,
viewer-rlva-floaters-toggles]
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

The RLVa lock model is structured bookkeeping beyond boolean restrictions,
kept in dedicated registries in `rlvlocks.cpp`: `@detach[:attachpt]=n`
locks worn attachments on (globally or per attachment point);
`@addattach[:attachpt]` / `@remattach[:attachpt]` lock attachment POINTS
against adding or removing (RlvAttachmentLocks); `@addoutfit[:layer]` /
`@remoutfit[:layer]` in their `=n`/`=y` sense lock wearable TYPES
(RlvWearableLocks); `@attachthis` / `@attachallthis` / `@detachthis` /
`@detachallthis` (`=n`/`=y`) lock shared-inventory FOLDERS
(RlvFolderLocks), with `@attachthis_except` / `@attachallthis_except` /
`@detachthis_except` / `@detachallthis_except` carving exceptions; and
`@unsharedwear` / `@unsharedunwear` gate items outside the #RLV tree.
Every Firestorm wear/detach path consults these registries, and the
RlvAttachmentLockWatchdog re-attaches a locked attachment the server
kicked off. Our side has only the parser (`sl-rlv/src/behaviour.rs`
recognises all these keywords) — no lock registries and no enforcement.

Scope: lock registries keyed off [[viewer-rlv-restriction-state]];
can-wear/can-remove predicates that the Session and outfit paths must
consult (overlapping the refuse-commands surface of
[[viewer-rlv-enforce-send-side]] and the force-wear matrix of
[[viewer-rlv-enforce-forced-actions]]); the re-attach watchdog; and the
lock-aware inventory-hiding settings (RLVaHideLockedLayers,
RLVaHideLockedAttachments, RLVaHideLockedInventory — surfaced by
[[viewer-rlva-floaters-toggles]]).

Reference (Firestorm, read-only): `indra/newview/rlvlocks.cpp`,
`indra/newview/rlvlocks.h`, `indra/newview/rlvinventory.cpp`.
