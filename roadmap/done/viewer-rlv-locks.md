---
id: viewer-rlv-locks
title: RLV — attachment, wearable and folder locks
topic: viewer
status: done
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

## Done (2026-09-06)

`sl-rlv/src/locks.rs` holds the lock model and `sl-rlv/src/watchdog.rs` the
re-attach watchdog.

The four registries are **derived**, not maintained: `RlvLocks::of(&state)`
(also `RlvState::locks()`) reads the held-command list, so there is no second
copy of the truth to drift and `clear_object` drops an object's locks with
nothing else to remember. What it derives:

- attachment-**point** locks from `@addattach[:pt]`, `@remattach[:pt]` and
  `@detach:<pt>` (which locks both ways), with a bare command modelled as
  `point: None` rather than 55 expanded rows;
- attachment-**object** locks from a bare `@detach=n`, which needs the one
  fact that cannot be derived — where the *issuing* object is worn. That is
  cached on the state machine by the new `RlvState::set_object_attachment`,
  mirroring the reference's `RlvObject` lookup, because `@detach=y` routinely
  arrives after the object is already gone;
- wearable-**layer** locks from `@addoutfit[:layer]` / `@remoutfit[:layer]`;
- **folder** locks from `@attachthis` / `@detachthis` (+ `_except`),
  `@sharedwear` / `@sharedunwear` and `@unsharedwear` / `@unsharedunwear` —
  the last pair modelled as the reference does, a whole-inventory DENY with
  `#RLV` punched back out by an ALLOW.

`is_folder_locked` walks up the tree: DENY locks outright, an ALLOW from some
object makes every later lock *from that same object* stop counting (which is
`@detachthis_except`, and why a second collar's lock survives the first one's
exception), a node-scoped lock counts only on the folder asked about, and a
folded folder (`.(chest)`, `.(nostrip)`) is its parent for locking. The
predicates `can_attach` / `can_detach` / `can_wear` / `can_remove` are the
honest implementation of the four `can_*` methods on `RlvQuerySource`, whose
docs now say so. `is_strippable` covers the `nostrip` naming convention, which
is not an RLV restriction at all — nobody issued it and nothing lifts it.

`RlvAttachmentWatchdog` models the reference's re-attach machine as a pure
state machine: `on_wear_requested` / `on_attach` / `on_detach` /
`on_asset_saved` / `tick`, returning `RlvWatchdogAction::Detach` and `Attach`
for the consumer to send, and an `allowed` flag for `@notify`. Time is a plain
seconds count on every call, so it is testable without a clock. It undoes three
things: a locked attachment taken off (waiting for the asset save, forcing at
15s, retrying at 30s), a wear onto an add-locked point (restored to exactly
what was there when the wear was *asked for*), and a replace onto a point
holding something locked (refused, or with `RLVaWearReplaceUnlocked` allowed to
displace only what was free).

Also new: `RlvBehaviourFlags::SUBTREE`, marking the sixteen `all` spellings —
`@attachallthis` beside `@attachthis`, `@attachall=force` beside
`@attach=force`. The scope of a folder lock is a property of the *keyword*, not
of the behaviour they share, which is exactly what the reference's
`FORCEWEAR_SUBTREE` is. The force-wear rows are flagged too even though only
the restriction rows are read yet; a half-filled table is worse than a full one.

One reference filter deliberately not reproduced, documented on
`is_folder_locked`: `RlvFolderLocks::isLockedFolder` takes a lock-*source* mask
and then tests it against the lock **type** (`rlvlocks.cpp:1181`). The two
enumerations share no bit meanings, so that filter does not do what its name
says; it is left out rather than copied.

Not verified live: no consumer yet, same as [[viewer-rlv-queries]] and
[[viewer-rlv-notify]]. Wiring `RlvLockSource` to the viewer's inventory and
appearance mirrors, and sending the watchdog's actions, belongs with
[[viewer-rlv-enforce-forced-actions]]; the `RLVaHideLocked*` settings are
surfaced by [[viewer-rlva-floaters-toggles]].
