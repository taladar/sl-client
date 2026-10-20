---
id: viewer-bake-cache-query
title: Ask a client-baking region's cache before re-uploading the own bakes
topic: viewer
status: done
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

- Publish the reference's per-bake hash as the slot's cache id:
  `sl_proto::avatar_texture::wearables_hash` (from
  [[server-bake-service]], 2026-10-20) is
  `LLWearableData::computeBakedTextureHash` — an MD5 of each contributing
  worn wearable's asset id in the bake's type order, then the bake's
  `mWearablesHashID`. It is **not** what Second Life names its bakes by (the
  census compared it on every slot), which does not matter here: only a
  client-baking region asks for it, and OpenSim compares the id it was
  published under.
- Before compositing, send the query and skip the upload of every slot the
  answer names, putting the named texture on the slot instead.
- Only where the region does not central-bake (Second Life never answers the
  query).

An e2e test against `FakeOpensim`: a second login with the same outfit
uploads nothing.

## Done (2026-10-20): scope changed

`appearance-baking` now logs A in again after the publish and asks the cache
under the published ids: the local OpenSim answers **no hits**, and the new
login arrives with the default placeholder on every slot. A stock region keeps
the cache in memory for the session only; only the optional XBakes service
(`[XBakes] URL`, unset by default and on the local grid) carries it into a
later login. So "a second login uploads nothing" is not what OpenSim does, and
the e2e test holds what it does instead: an outfit change re-uploads only the
slots whose hash it changed.

That needed what the task did not list:

- the viewer publishes once **per outfit** (it was a one-shot per session, so
  an outfit change on OpenSim was never shown to anyone): a new worn set
  starts a query → upload of the misses → `AgentSetAppearance` round;
- the fake grid takes an `AgentIsNowWearing` as the simulator's outfit
  (`ServerEvent::WearingSet`), so a wear reaches the next
  `AgentWearablesUpdate`;
- the census's relogin found the fake Second Life flavour resetting the
  Current Outfit Folder's version at each login (aditi keeps it): each
  account's version now outlives its session (`OutfitVersions`,
  `ServerEvent::CurrentOutfitVersionIncremented`);
- the conformance harness retries an "already logged in" on `fake-opensim`
  as it does on OpenSim.
