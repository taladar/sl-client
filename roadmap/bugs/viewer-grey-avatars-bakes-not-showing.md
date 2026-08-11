---
id: viewer-grey-avatars-bakes-not-showing
title: Most other avatars render grey (baked skin/textures not showing) on aditi
topic: viewer
status: bugs
origin: noticed live on aditi while extending F3 / async fetch (2026-08-11)
---

Context: [context/viewer.md](../context/viewer.md).

Live on aditi: nearly every **other** avatar renders **grey** (flat skin, no
baked textures) while the **own** avatar textures correctly. The user recalls
avatars were "much more complete before the performance branch merge", so a
perf-branch regression is suspected — and it is likely **more than one bug**.

## What is confirmed (not the cause)

- `agent_appearance_service` **does** parse (aditi returns
  `http://bake-texture.glb.aditi.lindenlab.com/`) — logged by the new sl-wire
  probe. `ingest_avatar_bakes` correctly takes the **server-bake** branch
  (`avatars.rs`), not the by-UUID CDN fallback.
- The bake service **works**: fetching the own avatar's head bake URL by hand
  returns `HTTP 200` (1.5 MB J2C). Own-avatar bakes load (and disk-cache).

## Distinct sub-causes seen

1. **Some grey avatars record ZERO visible bakes.** One avatar's
   `AvatarAppearance` re-processed ~15× in 30 s, each time
   `requested 0 baked texture(s)` — so `visible_body_bakes` returned empty and
   Tex Refresh has nothing to re-request. Why 0? (genuinely unbaked/cloud, a
   texture-entry parse gap, or `is_bake_visible` wrongly rejecting.) The
   **repeated re-processing** is itself suspicious — the COF-version gate
   (`should_refetch_bakes`) should suppress it, so the appearance likely carries
   no `cof_version`.
2. **Bake fetched OK but never applied (hypothesis).** User: "maybe an
   optimization prevents the *application* of the bake and it fetches okay."
   Application is `assign_avatar_bake_materials` / `apply_avatar_bake_textures`;
   an equality-guard / budget / debounce there could drop the drape.
3. **CDN 503 for other avatars' bakes.** 30 fetches failed
   `HTTP 503 Service Unavailable - DNS failure` (Akamai origin-resolution) with
   retries exhausted — an aditi/CDN-side origin failure for *those* assets,
   while own-avatar bakes succeed. Partly external, but our retry gives up
   permanently.

## Regression suspects (perf branch, 2a6484f5 area)

- `2a6484f5 "budget + debounce avatar appearance application"` — debounces the
  shape/morph re-apply; check whether it also starves / drops the bake-material
  application path (sub-cause 2).
- Also the T-pose symptom (other avatars stuck in T-pose, idle anim not driving
  the skeleton) — suspect `41609694 "pose gate — skip settled avatar/animesh
  skeleton evaluation"`. Filed here as a related but separate bug to split out.

## Levers already landed

- Manual **Tex Refresh** pie action (self + other) re-issues + evicts
  (`TextureManager::forget`) an avatar's bakes for another try — helps sub-cause
  3 when the CDN recovers, but is a no-op for sub-cause 1 (0 recorded bakes).
- Diagnostics: sl-wire `agent_appearance_service` log; per-slot server-bake vs
  by-UUID debug in `ingest_avatar_bakes`.

## Next

Reproduce with `RUST_LOG=sl_client_bevy_viewer::avatars=debug` on a crowd,
correlate a specific grey avatar to sub-cause 1/2/3, and bisect the perf branch
(`2a6484f5`) for sub-cause 2.

(The by-UUID fallback is **correct** on OpenSim after all — other avatars' own
viewers client-bake *and upload* the result, which we then fetch by UUID; only
SL uses the appearance service. So no fallback change is needed.)
