---
id: viewer-perf-avian-collider-tree-rez
title: Reduce avian3d collider-tree churn during bulk rez
topic: viewer
status: wont-do
origin: Tracy profiling of Aditi rezzing (2026-07-30)
refs: [viewer-perf-custom-static-raycast-index]
---

Context: [context/viewer.md](../context/viewer.md).

**Won't do (audit 2026-10-15): obsolete.** avian3d is no longer a dependency
of the viewer — [[viewer-perf-custom-static-raycast-index]] replaced it with a
custom parry3d static raycast index (`sl-viewer-kit/src/raycast_index.rs`),
so `block_on_optimize_trees` and the avian broadphase this item measured no
longer run. Any rez-time cost of the new index is that task's territory.
The original measurement is kept below.

Tracy self-time over the first ~10 s of rezzing on Aditi shows the avian3d
physics broadphase re-optimizing constantly as colliders stream in:

| System | ms/frame | n |
| --- | --- | --- |
| `avian3d::collider_tree::optimization::block_on_optimize_trees` | 1.09 | 454 |
| `ray culling` (avian broadphase) | ~1.0 | 968 |

(`n=454` over 204 frames ≈ the physics schedule running ~2×/frame.) Each newly
rezzed prim that gets a collider dirties the acceleration tree, so during a rez
burst the tree is re-optimized continuously — ~2 ms/frame of physics broadphase
work while the world fills in.

Investigate:

- Whether every rezzing prim needs a physics collider immediately, or collider
  creation can be **deferred / batched** (e.g. added after the rez burst
  settles, or only for prims the avatar can actually collide with — the same
  spirit as the flexi-settle and LOD work).
- Whether the collider-tree optimization cadence can be throttled during bulk
  insertion so it optimizes once after a batch rather than per insertion.
- What the colliders are even used for in the viewer (camera collision, avatar
  walking, pick rays) and whether a cheaper representation suffices for distant
  / just-rezzed content.

Measure `block_on_optimize_trees` + `ray culling` self-time before/after with a
≤10 s `tracy-capture` capture during an active rez.
