---
id: viewer-outfit-change-during-bake-is-lost
title: An outfit change during the server bake handshake is lost
topic: viewer
status: done
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

## Fix (2026-11-24)

Fixed as planned.

- `ServerBakeState::outfit_during_round` is set to `Changed` when an
  `AgentWearables` or a newer Current Outfit Folder descendents reply arrives
  while a round is under way, that is in `FoldersRequested` or `BakeRequested`.
- `finish_round` ends the round. It runs whether the bake was accepted or given
  up on, and if a change is pending it starts one more round by reading the
  outfit folder again.
- Two changes in one round lead to one more round, not two.

Test: `an_outfit_change_during_the_bake_bakes_again_after_it` in
`sl-viewer-world-avatar/src/appearance.rs`.

The folder version this re-read depends on can no longer move backwards, because
[[inventory-stale-folder-fetch-overwrites-newer-state]] is fixed too.
