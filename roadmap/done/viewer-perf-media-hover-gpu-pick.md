---
id: viewer-perf-media-hover-gpu-pick
title: Media-hover off the per-frame MeshRayCast onto the GPU pick
topic: viewer
status: done
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

## Outcome (2026-08-15): DONE

Implemented the GPU-pick direction. Added a `PickPurpose::Media`;
`hover_media_faces` now requests a Media pick at ~`PICK_HZ` while the cursor is
over world content and consumes `GpuPickResolved` to remember the media face the
pick landed on (the nearest visible thing under the cursor, so occlusion is
correct by construction — the pick renders the real posed scene). The per-frame
ray survives only as a **single-mesh** cast filtered to that one entity (the
pick's `ObjectFace` "surface-refinement ray test"), purely to read the current
surface UV for `mouse_move` forwarding — no whole-scene `MeshRayCast`. All the
existing UV / interaction / `mouse_leave` logic is unchanged.

So the 54 ms/frame whole-scene cast is gone: steady state is the O(1) pick plus,
only while actually hovering a media face, a one-mesh UV cast.

Live-verified on OpenSim: hovering a video-media prim still shows the media
controls (user confirmed). Occlusion suppression follows by construction from
the pick (the nearest-visible resolution); the crowd-scale perf number was not
re-captured (the win is structural — the unfiltered cast is deleted).
