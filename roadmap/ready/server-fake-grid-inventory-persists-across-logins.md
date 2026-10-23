---
id: server-fake-grid-inventory-persists-across-logins
title: Fake grid — keep an account's inventory, versions included, from one login to the next
topic: server
status: ready
origin: gridspec-inventory-fetch (2026-10-21)
refs: [gridspec-inventory-fetch, server-fake-grid-inventory-udp-mutations]
blocked_by: [gridspec-inventory-fetch]
---

Context: [context/server.md](../context/server.md).

## What

Both live grids keep what a session changed: the next login's skeleton states
the versions the last session read, and a fetch agrees with it
(`book/src/gridspec/inventory.md` § Fetching, `inventory-fetch`, 2026-10-21 —
`relogin_skeleton_root_step`, `relogin_skeleton_trash_step` and
`relogin_fetch_root_step` all 0 on aditi and OpenSim). The fake grid seeds each
login's serving tree afresh from the fixtures, so every folder the last session
made is gone and the root's version falls back (by 5 on `FakeOpensim` and 7 on
`FakeSl` in the census). Only the Current Outfit Folder's version is carried
over today (`OutfitVersions`, for the bake legs).

## How

Keep each account's serving tree on the grid when a session ends (logout, or
the circuit dropping) and seed the next login's `SimSession` from it instead of
the fixtures, as `outfit_versions` does for the one folder — which this then
replaces. Mind the region sessions: a teleport or crossing hands the avatar to
another region's `SimSession`, so the tree must move with the avatar rather
than live in one region.

Then drop the `relogin_` row of `FAKE_EXEMPT` in
`sl-conformance/src/cases/inventory_fetch.rs`, so both flavours are held to the
live answer.

## From gridspec-notecards-gestures (2026-10-23)

The `notecards-gestures` census reads its gestures back through the item
fetch after a relogin (`relogged_active_*`, `relogged_deactivated_*`); on the
fake grid they are gone, so `FAKE_EXEMPT` in
`sl-conformance/src/cases/notecards_gestures.rs` exempts the `relogged_` rows.
Drop that row too. The account's active gestures are kept on the grid
already (`crate::notecards::ActiveGestures`, like `OutfitVersions`); once the
tree persists they could live in the items' own state, as OpenSim's flag
does.
