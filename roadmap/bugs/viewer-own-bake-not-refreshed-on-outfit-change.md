---
id: viewer-own-bake-not-refreshed-on-outfit-change
title: Own avatar bake not refreshed when worn layers change at runtime
topic: viewer
status: bugs
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
