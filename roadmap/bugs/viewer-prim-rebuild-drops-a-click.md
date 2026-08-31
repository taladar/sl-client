---
id: viewer-prim-rebuild-drops-a-click
title: A prim's re-tessellation frame swallows any click on it
topic: viewer
status: bugs
origin: found while writing the pie-target negatives (2026-08-31)
points: 3
refs: [viewer-world-pie-target-tests, viewer-cpu-pick-resolver]
---

Context: [context/testing.md](../context/testing.md).

When a prim's faces are rebuilt, the old face entities are despawned and
the new ones spawned, and for **one frame** the prim has no face entities
at all. A pick resolved on that frame hits nothing, so the click that
asked for it is silently dropped: no pie, no touch, no selection — and
nothing anywhere says why.

Measured in the fixture world ([[viewer-world-test-harness]]) with a
per-frame census of `With<MeshTag>` face entities. The fixture prim's
faces are entity `266v0` on frames 2–16, **no faces on frame 17**, and
entity `271v0` from frame 18 — a rebuild about ten frames after the
camera lands. A right-click whose release falls on frame 17 requests its
pick normally (the gesture guards all pass, the ray is aimed correctly at
the prim), and `resolve_cpu_picks` answers with `hit: None`. Frame 16 or
18: exactly one pie. This is not the CPU resolver's doing — the GPU
resolver rasterises the same absent geometry — and not the fixture's:
any LOD switch, mesh arrival or texture-driven re-mesh does the same
thing to a live prim.

The user-visible shape is a rare "the menu just didn't open" — one
dropped click in the frame a prim happens to re-mesh, most likely
while a region is still rezzing, which is exactly when a user clicks
around most.

Worth fixing at the rebuild, not at the pick: build the new faces before
despawning the old ones (or keep the old entities and swap their meshes,
which the material-rebind rules already push us toward), so a prim is
never *absent*, only momentarily stale. A pick that resolves to the
about-to-be-replaced face still names the right object, which is all the
menus need.

The two pie negatives ([[viewer-world-pie-target-tests]]) settle past
this frame deliberately and say so: a negative that landed on it would
pass for the wrong reason.
