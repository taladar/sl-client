---
id: viewer-outfit-change-during-bake-is-lost
title: An outfit change during the server bake handshake is lost
topic: viewer
status: bugs
origin: mixed-channel race audit (2026-11-23)
refs: [inventory-stale-folder-fetch-overwrites-newer-state]
---

Context: [context/viewer.md](../context/viewer.md).

Confirmed by reading the code. Not reproduced live. Applies to Second Life's
server baking.

## Bad order

The bake is requested at outfit version N, the outfit changes (UDP
`AgentWearablesUpdate` or the outfit folder's refetch), then the
`UpdateAvatarAppearance` HTTP reply accepts N.

## Cause

Both re-bake triggers in `sl-viewer-world-avatar/src/appearance.rs` require
`BakeStage::Done`. The comment says the in-flight bake reads the latest
version, which stops being true once its request has been sent.

## Effect

Other residents keep seeing the old outfit until the next outfit change.

## Fix

Set a pending flag when a change arrives mid-handshake, and run the bake again
when the stage reaches `Done`.
