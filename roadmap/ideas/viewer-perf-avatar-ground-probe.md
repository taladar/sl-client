---
id: viewer-perf-avatar-ground-probe
title: Avatar ground probe — stop per-frame full-scene raycasts
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

`probe_avatar_ground` (`ground.rs:149-219`) casts, **every frame, for
every rigged avatar**, three vertical `MeshRayCast` rays (root + both
ankles). Bevy's `MeshRayCast` iterates the whole scene of mesh entities
per cast, and the `accept` closure (`ground.rs:172-188`) walks each
candidate's parent chain calling `avatar_roots.contains(&current)` — a
linear scan of a `Vec` rebuilt each frame (`ground.rs:166-169`).

Cost ≈ avatars × 3 rays × scene-mesh count × parent-chain depth ×
avatar-count (the linear `contains`), per frame. On a dense region
(thousands of prim faces) with several avatars this is among the heaviest
per-frame CPU items, and it grows super-linearly: avatars appear both as
ray sources and inside the `contains` scan.

## Proposed fix (three independent layers)

1. **`HashSet<Entity>` (or a marker component) instead of the `Vec`
   scan.** Trivial, safe, removes the inner linear factor. Better still:
   a precomputed "is avatar geometry" marker on the mesh entities so the
   accept closure needs no parent walk at all.
2. **Throttle.** Feet need re-grounding a few times per second, not at
   frame rate — follow the `drive_render_priority` self-throttle shape
   (`render_priority.rs:65`, 4 Hz). Cache the last probe result between
   ticks. Mind the foot-IK constraint (memory
   `sl-client-foot-ik-near-singular-leg`): the probe must never read the
   posed pose, and flat ground must stay a no-op — a stale-but-valid
   cached height satisfies both.
3. **Distance cut.** Only probe the own avatar and near avatars (we
   already track avatar distances for interest/priority); distant
   avatars' foot placement is sub-pixel.

## Estimated impact

High; this is the survey's top per-frame CPU finding. Scales down from
`O(avatars × scene)` per frame to `O(near-avatars × scene)` per throttle
tick — on a 20-avatar, prim-dense scene plausibly several ms/frame
recovered. The HashSet change alone is a cheap guaranteed win even
before throttling. Verify with [[viewer-profiling]] Tracy zones around
the probe (span already isolatable per system).

Confidence: high (code path and registration verified; no run condition
on the system).
