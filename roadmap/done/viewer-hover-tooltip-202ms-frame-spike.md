---
id: viewer-hover-tooltip-202ms-frame-spike
title: update_hover_tooltip spikes to 202 ms on a single frame
topic: viewer
status: done
origin: GPU-avatar Phase 4 perf capture (2026-08-13)
refs: [viewer-perf-hover-pick-raycast, viewer-perf-gpu-avatar-phase3-gpu-picking]
---

Context: [context/viewer.md](../context/viewer.md); code
`sl-client-bevy-viewer/src/hover_tooltip.rs`.

In a 27.6 s / 512-frame aditi tracy capture, `update_hover_tooltip` had
**mean 0.5 ms but max 202.9 ms** on a single frame (one instance out of 509).
That is abnormal: there is only ever **one** hover tooltip, so this system
should be trivial and near-constant. A 200 ms hitch in it is a
single-digit-FPS frame on its own.

## Why it's suspicious

Phase 3 moved hovering onto the async GPU ID-buffer pick
([[viewer-perf-gpu-avatar-phase3-gpu-picking]]), which is supposed to have
retired the old `MeshRayCast` cost ([[viewer-perf-hover-pick-raycast]]) and be
~0. A 202 ms spike means something occasionally does heavy work on this path.

## Candidate causes (to confirm by repro + a zoom on that frame)

- A **synchronous stall** somewhere on the hover path (a blocking
  readback/map wait, or a fallback that still ray-casts) on the frame the
  pick pipeline is still compiling or the readback is late.
- A **tooltip UI rebuild** — despawn+respawn of the tooltip widget/text on
  content change (the "build once, update in place" rule — see
  `sl-client-floater-build-once-update-in-place`); a full text relayout of a
  large tooltip would show here.
- Coincidence with an **asset-streaming stall** (see the sibling perf task):
  the spike may just be this system caught behind a big upload on the same
  frame — rule this out first by checking whether the 202 ms is *self* time or
  inherited wait.

## Verify

Reproduce with a tracy capture, find the single spiking instance, zoom to that
frame, and read this zone's **self** time and children. If self-time is high,
fix the heavy work (make it constant-time / build-once). If it's inherited
wait, reclassify as the asset-streaming spike, not a hover bug.

## Resolution (2026-09-17)

Re-captured on aditi (release, `profile-tracy`, whole-session captures, the
pointer resting on objects, avatars, land and across a region border). The
202 ms instance did not reproduce, but the system's whole cost was work it
did not need, all of it on the dwelt path:

1. **A whole-scene broad phase for the HUD-occlusion test.** Bevy's
   `MeshRayCast` runs its AABB `par_iter` over **every** mesh before it
   consults the filter, so "is a HUD under the cursor?" swept the region on
   every dwelt frame, and its join waited on whatever the compute pool was
   busy with — the plausible source of a rare huge spike. Replaced by
   `sl_viewer_world_api::targeted_ray_cast::TargetedRayCast` (a ray cast over
   a named candidate set) behind `hud_pick::HudRayCast`, whose candidates are
   the `HudScreen` subtree. (Gathering them by `RenderLayers` instead was
   measured still ~0.5 ms: world geometry carries the main and probe layers.)
   The same helper replaces the single-face `MeshRayCast` refinements
   (`resolve_touch_pick`, `ObjectPicker::pick_entity`), which also swept the
   scene despite the "not a scene walk" comment.
2. **A linkset count per frame.** `ObjectState::linkset_prim_count` scans
   every tracked object; the tip now counts once per hovered root and
   recomposes its lines only at the pick rate or on a target change.

`update_hover_tooltip` per instance:

| capture | mean | p50 | p90 | p99 | max |
| --- | --- | --- | --- | --- | --- |
| before | 650 µs | 732 µs | 1487 µs | 2356 µs | 8.1 ms |
| targeted HUD cast | 447 µs | 54 µs | 1164 µs | 1800 µs | 10.0 ms |
| + linkset count once | 179 µs | 14 µs | 610 µs | 1184 µs | 1.6 ms |
| + `HudScreen` subtree | 21 µs | 9 µs | 41 µs | 255 µs | 0.7 ms |

### Found on the way: neighbour-region objects never resolved

Hovering an object across a region border showed "Loading…" forever: every
object-key request (`RequestObjectPropertiesFamily`, `RequestPayPrice`, spin,
grab update, script running / reset, `BuyObjectInventory`) went out on the
**root** circuit, and `dispatch_child` dropped the replies anyway. Now routed
to the object's own circuit (`Session::circuit_for_object`, the reference's
`objectp->getRegion()->getHost()`), with the replies and a neighbour script's
`ScriptDialog` / `ScriptQuestion` handled on child circuits and the script's
answer sent back where the request came from
(`Session::circuit_for_script_reply`). Verified live on aditi: cross-border
tooltips resolve. Left for their own items:
[[viewer-neighbour-object-caps-use-root-region]] (land impact via
`GetObjectCost`) and [[viewer-sit-on-neighbour-object-uses-root-circuit]]; a
P-key probe during the run also turned up
[[viewer-texture-rotation-offset-t-in-flipped-uv-space]].
