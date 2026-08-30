---
id: viewer-cpu-pick-resolver
title: A CPU pick resolver — the ID-buffer render's headless double
topic: viewer
status: ready
origin: test-harness plan (2026-08-30) — found while specifying the fixture world
points: 3
refs: [viewer-world-test-harness]
blocked_by: [viewer-plugin-groups]
---

Context: [context/testing.md](../context/testing.md).

World picks (touch, right-click pies, double-click, hover, drag) are a
GPU ID-buffer render: `GpuPicker::request` → `submit_gpu_picks` → a
readback observer → `GpuPickResolved`. Only the edit click-select and the
HUD/gizmo casts use `MeshRayCast`. A fixture world without a renderer
therefore cannot classify a right-click target at all.

Add `CpuPickResolverPlugin` beside the GPU one: the same `PickRegistry`,
the same `GpuPickResolved` channel, a `MeshRayCast` (filtered to tagged,
non-HUD entities, inherited visibility) in place of the rasteriser.
`GpuPicker::take_requests()` is shared by both. Classification — avatar
vs object face vs terrain vs water, worn-attachment routing — stays the
real code, which is what the interaction tier is testing.

Teeth: a request beside every fixture resolves to `None`; a request on
the fat prim resolves to its face with a world point on its near surface.
