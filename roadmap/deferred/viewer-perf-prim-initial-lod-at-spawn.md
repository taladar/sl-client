---
id: viewer-perf-prim-initial-lod-at-spawn
title: Build a plain prim at its warranted LOD instead of Low-then-refine
topic: viewer
status: deferred
origin: asked while settling viewer-prim-rebuild-drops-a-click (2026-09-04)
refs:
  [
    viewer-p21-3,
    viewer-perf-lod-apply-budget,
    viewer-prim-rebuild-drops-a-click,
    viewer-object-face-entity-respawn-churn,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

Every client-tessellated object is spawned at `INITIAL_MANAGED_PRIM_LOD`
(`PrimLod::Low`) and re-tessellated once `drive_render_priority` has sized it
against the camera. So a prim the camera is looking at is built **twice**: once
coarse, once at the level it actually warrants. The question this task records
is whether the first build can be skipped.

## Not by delaying the build

The obvious form — hold the tessellation until the driver has ranked the object
— is the wrong shape. `drive_render_priority` runs on a timer, not per frame:

```text
const REPRIORITIZE_INTERVAL_SECS: f32 = 0.25;
```

so a prim would be invisible for up to a quarter second after it arrives, and
every prim of a rez burst at once. A hole in the world during exactly the moment
that should look smooth.

## But the level could be known at spawn

Nothing in the plain-prim branch of the driver reads the built geometry — the
face query feeds only the *texture* area aggregation. The prim's own level is

```text
PrimLod::for_distance(scale_length, distance, lod_factor)
```

where `scale_length` is the object's scale vector length (`Object.scale`, known
at spawn) and `distance` is the camera distance. `apply_object` has both. It
could build once, at the right level, with no delay and no invisible window.

## What it would actually save

- **Less than it looks for repeated shapes.** `GeometryCache` is keyed
  `(shape, lod)` and shares mesh handles across instances, so a region of
  default boxes tessellates `Low` once and revives it for the rest — the waste
  per instance is a face-entity build and a material intern, not geometry work.
  A varied or one-off shape does pay a genuinely wasted tessellation.
- **The clearer cost is budget.** Both the first build and the refine spend a
  `MeshUploadBudget` slot ([[viewer-perf-lod-apply-budget]]), so a rez burst
  pays twice per prim and spreads over more frames than it needs to.

## What it would cost

- **It partly undoes a deliberate policy.** `INITIAL_MANAGED_PRIM_LOD`'s own
  doc argues that starting coarse "keeps a dense region's initial geometry
  small and only refines the prims the camera looks at". Ranking exactly at
  spawn front-loads full-detail tessellation for everything arriving near the
  camera — the spike the placeholder exists to avoid.
- `update_objects` would need the camera and the LOD-factor setting, plus a
  `Low` fallback for "no camera yet" (login, teleport).
- **Linkset children rank wrong.** At spawn only the local `Transform` exists,
  not the propagated `GlobalTransform`, and a child's is parent-relative — and
  a child can arrive before its root. Exactly the objects that arrive out of
  order would get the wrong distance, so children would have to fall back to
  `Low` and let the driver fix them.

The shape worth building, if it is built: compute at spawn **only** for a root
prim, with a camera present, close enough that the driver's next pass would
refine it anyway; everything else keeps `Low`. That removes the double build
for the prims being looked at — the only ones that get refined — without
front-loading the region.

## Why deferred rather than ready

It wants a measurement first: on a live region, how many prims actually take
exactly one refine, and what share of a rez burst's `MeshUploadBudget` goes on
builds that are immediately replaced. Without that the added coupling in
`update_objects` (camera + settings + a root/child split) is being paid for a
saving nobody has sized.

Independent of the rebuild path's correctness: the camera re-ranks continuously,
so prims cross tiers for the rest of a session and
[[viewer-prim-rebuild-drops-a-click]]'s in-place rebuild stays load-bearing
whatever happens here.
