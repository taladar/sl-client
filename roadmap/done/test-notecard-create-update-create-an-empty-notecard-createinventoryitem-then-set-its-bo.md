---
id: test-notecard-create-update
title: create an empty notecard (CreateInventoryItem), then set its body over
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 13 — Asset & texture pipeline `[both]`
---

Context: [context/test.md](../context/test.md).

`notecard-create-update` — create an empty notecard
(`CreateInventoryItem`), then set its body over the
`UpdateNotecardAgentInventory` capability
(`Command::UpdateInventoryAsset` with [`UpdatableAssetType::Notecard`]).
`1av` `[both]`. The Second Life way to author a notecard — and the flow
`asset-upload` cannot exercise on SL, since `NewFileAgentInventory` refuses
the notecard class. Both grids offer the create-then-update path (OpenSim also
accepts the one-step `NewFileAgentInventory` notecard `asset-upload` uses, but
SL does not), so this covers the portable path on both. The create rides UDP
`CreateInventoryItem` (the `UpdateCreateInventoryItem` reply allocates the
item id and a placeholder body asset); the body write is the two-step CAPS
POST to `UpdateNotecardAgentInventory`, whose `AssetUploaded` names the new
body asset that replaces the placeholder. The case asserts the sim-approved
created item, the non-nil new body asset, then best-effort re-fetches that
asset over the `ViewerAsset` `AssetStore` to confirm the body round-trips, and
deletes the item. `complete` on **both** grids — unlike `asset-upload`, this
is the SL-native path so aditi is green (create + update both succeed;
`roundtrip = match` on OpenSim, `skipped` on aditi where `ViewerAsset` 503s,
as in `asset-fetch-http`). Added the missing `UpdatableAssetType` re-export to
both runtime crates (`sl-client-tokio` / `sl-client-bevy`).
