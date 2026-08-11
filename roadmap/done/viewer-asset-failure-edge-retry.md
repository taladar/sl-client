---
id: viewer-asset-failure-edge-retry
title: Retry failed asset fetches and make the F3 overlay honest about deferred work
topic: viewer
status: done
origin: stuck-loading / "F3 shows nothing to load but items missing" audit (2026-08-11)
---

Context: [context/viewer.md](../context/viewer.md).

The F3 pipeline overlay reads only each store's `stats()`, and the stores keep
**weak** references, so any entry whose strong `Arc` drops is swept and reads as
all-zero. Combined with a failed fetch / decode (`None`) being treated as
terminal — for textures not even recorded as failed — a transient `GetTexture` /
`GetMesh` 503, a connection reset, or a decode blip on a **one-shot** consumer
(terrain detail texture, avatar bake, a static mesh whose one object update
already arrived) left the asset missing for the whole session while F3 showed
"nothing left to load". This is the general class behind the observed
"item / avatar bit clearly missing but F3 says nothing to load".

Fixes (all 2026-08-11):

- **Bounded retry, textures + meshes.** New `asset_retry` module (exponential
  backoff `0.5 → 30 s`, `MAX_RETRY_ATTEMPTS = 6`). `poll_textures` /
  `poll_meshes` now re-issue a failed fetch at the same parameters instead of
  giving up, keeping parked faces / pending objects in place until it succeeds;
  only after the retry budget is exhausted do they announce the failure
  (releasing faces to their fallback). Unit-tested pure logic
  (`backoff_doubles_then_caps`, `after_failure_counts_up_then_gives_up`).
  **Not yet live-verified** — a transient 503 is not reproducible on demand; the
  retry is conservative and self-limiting.
- **F3 honesty.** `TextureManager` / `MeshManager` expose a `deferred_count`
  (cap-parked + retry-pending), shown in the overlay as `defer N`, so work the
  weak-referenced store cannot see is no longer invisible.
- **Terrain lost-wakeup.** `learn_composition` now seeds already-decoded detail
  textures inline (a prim face may have fetched the same default Linden UUID
  earlier, so the store's one-shot `TextureDecoded` fired before the region
  existed), instead of the ground staying on the olive placeholder.
- **`unavailable` cleared on region cross.** `materials` / `animations` /
  `environment_assets` / `sound_cache` re-arm their permanent-`unavailable` set
  into the parked set on a capability refresh, so a post-cap transient failure
  (a region-cross fetch race, a `ViewerAsset` 503) recovers instead of stranding
  a face on the neutral white default (etc.) for the session.

Related still-open perf lever: [[viewer-perf-async-asset-fetchers]] — the
blocking-fetch-on-`IoTaskPool` design caps concurrent downloads at ~4 regardless
of the store gate, which is why F3 rarely shows the gate saturated.
