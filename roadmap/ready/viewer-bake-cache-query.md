---
id: viewer-bake-cache-query
title: Ask a client-baking region's cache before re-uploading the own bakes
topic: viewer
status: ready
origin: gridspec-appearance-baking (2026-10-19)
refs: [gridspec-appearance-baking]
---

Context: [context/viewer.md](../context/viewer.md).

On a region that does not central-bake, the reference first asks the region's
cache which of its bakes it already holds
(`LLAgentWearables::queryWearableCache`: an `AgentCachedTexture` of each
baked slot under `computeBakedTextureHash`, the hash of the wearables
composited into it) and only uploads the slots that miss. OpenSim answers
every such query, one entry per slot, and names a bake for each slot whose
cache id an earlier `AgentSetAppearance` published
(`book/src/gridspec/appearance.md` § Baking, measured by
`appearance-baking`).

Our viewer (`sl-viewer-world-avatar/src/bake_publish.rs`) never asks: it
composites, encodes and uploads every region at each login, and publishes a
synthetic per-slot cache id (`cache_id_for`) rather than the wearables hash,
so a later query could not hit even if it were sent.

## Work

- Compute the reference's per-bake hash from the worn wearables
  (`LLWearableData::computeBakedTextureHash`: an MD5 over what each
  contributing wearable adds in `addToBakedTextureHash`, then the bake's
  `mWearablesHashID`) and publish it as the slot's cache id.
- Before compositing, send the query and skip the upload of every slot the
  answer names, putting the named texture on the slot instead.
- Only where the region does not central-bake (Second Life never answers the
  query).

An e2e test against `FakeOpensim`: a second login with the same outfit
uploads nothing.
