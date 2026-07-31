---
id: viewer-own-bake-not-refreshed-on-outfit-change
title: Own avatar bake not refreshed when worn layers change at runtime
topic: viewer
status: done
origin: user report (2026-07-31, own avatar on aditi)
refs: [viewer-p15-3, viewer-p14-3, viewer-bom-mesh-alpha-feet-through-boots]
---

Context: [context/viewer.md](../context/viewer.md).

## Symptom

Changing our own avatar's worn **layers at runtime** — e.g. **taking off an
alpha layer** — does **not** produce a new bake: the old composited texture
persists, so a region the removed layer had carved stays hidden / unchanged
instead of revealing skin again. (Found while verifying
[[viewer-bom-mesh-alpha-feet-through-boots]] — the mask is correct for the
*current* bake, but the bake never updates.)

## Where to look

A rebuild path exists on both sides, so the bug is likely a missing **trigger**,
not a missing mechanism:

- **Local composite:** `OwnLocalBake` has a `built` flag (`avatars.rs:2852`)
  with a reset (`~:2860`), and `OwnBakeInputs::reassemble`
  (`bake_inputs.rs:169`) clears it. Does a runtime wearable change actually call
  `reassemble` (and
  re-fetch the newly-worn / drop the removed layer textures), or does
  `local.built` stay `true` so `apply_own_local_bake` never re-composites?
- **Server bake:** re-fetch is COF-version gated — `should_refetch_bakes(seen,
  cof)` against `state.baked_cof_version` (`avatars.rs:2359-2408`). On a runtime
  outfit edit, do we (a) receive/ingest a fresh `AvatarAppearance` for our own
  avatar with a **bumped `cof_version`**, and (b) re-request the new baked
  textures? If the version does not bump (or no new appearance arrives), the old
  `baked_textures` entry stands and `apply_bom_face_materials` keeps resolving
  the stale bake.
- Whether our own outfit edit even asks the sim to re-bake (AgentSetAppearance /
  the wearable-update round-trip), vs. only mutating local state.

## Verify

Live on aditi: with the avatar rezzed, remove an alpha layer (or swap a clothing
layer) and confirm the affected region re-textures / un-hides within a few
seconds, on both the server-bake and local-composite paths.

## Done (2026-07-31)

Both paths now re-bake on a runtime `AgentWearables` update.

- **Server bake** (`appearance.rs`): `drive_server_bake` re-runs the handshake
  on `AgentWearables` (and, for COF-only BoM-layer changes that never touch the
  legacy wearable set, when the Current Outfit Folder re-fetches) — the grid's
  COF-version mismatch recovery corrects a stale read. Verified live on aditi
  (COF advances, re-bake accepted per outfit change).
- **Local composite** (`bake_inputs.rs` / `avatars.rs`): a runtime
  `AgentWearables` re-fetches the worn assets and re-assembles; a `generation`
  counter drives `apply_own_local_bake` to re-composite. Verified on OpenSim.
- **Initial-bake race fix:** the runtime re-fetch only fires once the pipeline
  is **settled** (`Ready`); a change that lands mid-fetch is **deferred**
  (latest wins) rather than resetting the in-flight fetch — OpenSim sends a
  second `AgentWearables` during the initial fetch, and resetting mid-fetch let
  a stale asset-fetch event assemble an empty bake ("assembled from 0
  wearables"). Found and fixed live (user-diagnosed).
- **OpenSim add-not-rebake:** `AgentWearablesUpdate` decode now resolves a nil
  worn-item `asset_id` from the inventory cache, so a freshly added layer is not
  dropped from the composite. Verified on OpenSim.

Cross-cutting perf/inventory work landed in the same change (see
[[viewer-perf-avatar-bake-apply-spikes]] and the AIS3 inventory routing).
