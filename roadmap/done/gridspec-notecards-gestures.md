---
id: gridspec-notecards-gestures
title: Notecards, embedded items and gestures on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, test-asset-save-mutation-survey, viewer-gesture-runtime]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim stores a new notecard as one zero byte, SL a valid empty text; OpenSim
flips gesture item flags, SL only reports active gestures at login.

## Discover

`notecard_create_update`, `gestures`, `CopyInventoryFromNotecard`, embedded-item
saves, the login gestures array after activation.

## Document

`book/src/gridspec/inventory.md` § Notecards and gestures.

## Fake grid

Small — placeholder body, copy-from-notecard, gesture flags / login array per
flavour.

## Viewer

Restore active gestures per grid.

## Done (2026-10-23)

**Discover.** The census `notecards-gestures` (one avatar,
`sl-conformance/src/cases/notecards_gestures.rs`) measured a new notecard and
gesture, a saved body, an embedded item and a made-up one, the copies out of
a notecard, and gesture activation through two relogins (one not asking for
`gestures`, one asking); four runs on each live grid, the last two of each
identical, and the `MEASURED` table generated from them (101 rows). Findings:
Second Life serves **no notecard over `ViewerAsset`** (`503`, a gesture
saved in the same run reads `200`); it rewrites a saved notecard's embedded
item (a new id, group / everyone / next-owner masks `0x7fffffff`,
`0x7fffbfff`, `0x7fffffff`) and refuses one naming an asset the saver holds
no item of ("Internal error from verifier."); a new gesture there is the
"/hey" default; it keeps no item flag for an active gesture and sends the
login's `gestures` array only when asked. OpenSim stores one zero byte for a
new notecard or gesture, flags an active gesture `0x1`, answers a refused
copy `400` with an alert, and copies an item of the agent's under the asked-for
id as itself.

**Document.** `book/src/gridspec/inventory.md` § Notecards and gestures,
linked from `content/inventory.md` and `content/appearance.md`.

**Fake grid.** `sl-fake-grid/src/notecards.rs`: `CopyInventoryFromNotecard`
per flavour (now a `CapsDispatch::CopyFromNotecard` the driver answers, with
`SimCaps::copy_from_notecard_response`), the account's active gestures kept
across logins and stated in the login array (OpenSim's item flag too), the
starting bodies; `sl-proto/src/sim_notecard.rs` verifies and rewrites a
Second Life notecard save; the agent item UDP transfer is served; the Second
Life flavour refuses `notecard_id` over `ViewerAsset`
(`AssetHttpPolicy::refused_keys`). Both flavours pass the census offline,
but for the `relogged_` rows the fake grid cannot keep
([[server-fake-grid-inventory-persists-across-logins]]). Not modelled: a copy
out of a notecard in an object's contents (answered as finding nothing).

**Viewer.** The notecard editor reads a notecard through its item over the
UDP transfer (`Command::FetchInventoryItemAsset`, new in both runtimes and the
REPL, or `FetchTaskItemAsset`), opens one with no asset empty, and shows the
unreadable status on a refused transfer (`Event::TransferFailed` now names
the item). The client asks for `gestures` at login and hands the array over
as `Event::ActiveGestures`, which seeds the inventory's active-gesture set.
e2e: `a_notecard_reads_on_each_grid`,
`an_active_gesture_is_active_after_a_relog`.
