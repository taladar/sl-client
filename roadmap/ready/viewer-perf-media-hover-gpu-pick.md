---
id: viewer-perf-media-hover-gpu-pick
title: Media-hover off the per-frame MeshRayCast onto the GPU pick
topic: viewer
status: ready
origin: GPU-avatar crowd critical-path analysis (2026-08-14)
refs: [viewer-perf-gpu-avatar-phase3-gpu-picking, viewer-perf-gpu-avatar-extract-skins-floor]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md) §6 (GPU picking —
explicitly lists deleting the hover `MeshRayCast`).

`sl_client_bevy_viewer::media_prim::hover_media_faces` (which shows the media
control bar over a media-on-prim face under the cursor) runs a
**`MeshRayCast` every frame with `MeshRayCastSettings::default()` — no filter**,
so it intersects **every** mesh in the scene. `MeshRayCast` has no broad phase,
so the cost is `O(scene meshes)`.

Measured on the critical-path pass (aditi, `SL_VIEWER_CROWD=100`,
~4,500 skinned submeshes): `hover_media_faces` is **54 ms median / 117 ms p90**
— the single largest system in the frame, ~half the ~114 ms main-app frame,
bigger than `render_system`. It runs every frame even when the cursor is nowhere
near media. A real 40–100-avatar club inflates it the same way (that many worn
mesh submeshes), and a prim-dense region inflates it without any avatars.

## Why not just filter the cast to media faces

The full cast is load-bearing for **occlusion**: it takes the *nearest* hit and
shows the controls only if that nearest hit is a `MediaFace` (`media_prim.rs`
~847–852), so an avatar or wall in front of a media screen correctly suppresses
the controls. Filtering the cast to media faces alone (tiny set — media prims
are rare) would be maximally cheap but would show the controls **through** any
occluder. (Note the current avatar occlusion is already only approximate:
`MeshRayCast` tests the bind-pose mesh at the entity, not the GPU-posed
geometry.)

## Direction — use the GPU pick

The Phase 3 GPU pick (`gpu_pick.rs`) already renders the real scene's IDs +
depth under the cursor with **correct occlusion**, resolving the entity via
`PickResolution` (object faces carry their `PrimFaceEntity`) and unprojecting
the depth to a world hit point. Media hover should consume that instead of
casting: the entity under the cursor is an O(1) readback, occlusion is free
(posed avatars included), and the media path keeps only the cheap per-hit work
(resolve `MediaFace`, compute the face UV from the hit point for `mouse_move`
forwarding). This is the §6 "delete the hover `MeshRayCast`" intent, scoped to
media.

Interim cheap option if the full migration is deferred: exclude skinned meshes
from the cast (`with_filter`) — kills the 4,500-submesh cost, keeps full prim
occlusion, drops only the already-approximate avatar occlusion. Still casts
against all non-avatar prims, so it is a partial win, not the endgame.

## Verify

`CROWD=100` aditi (or a media-prim region): `hover_media_faces` drops from
~54 ms to sub-millisecond; media controls still appear over a media face, still
suppressed when an avatar / prim occludes it, and click-through / `mouse_move`
forwarding still work.
