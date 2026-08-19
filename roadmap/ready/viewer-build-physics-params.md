---
id: viewer-build-physics-params
title: Features tab — physics shape type & material params
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-prim-parameter-editing, viewer-build-display-options]
---

Context: [context/viewer.md](../context/viewer.md).

The Features tab's physics block: the **Physics Shape Type** combo
(Prim / Convex Hull / None) and the **Gravity / Friction / Density /
Restitution** spinners. Our Features tab
(`sl-client-bevy-viewer/src/edit_params.rs`) has no physics fields at
all.

The read side already exists (`Command::RequestObjectPhysicsData`,
`Event::ObjectPhysicsProperties`), but the write side is missing: our
ObjectFlagUpdate sender hardcodes `extra_physics: Vec::new()`
(`sl-proto/src/session/circuit.rs:4867` — the message's ExtraPhysics
block is where shape type and the four material params travel). This
task fills that block from a real command and adds the UI, following
the reference commit path (llpanelvolume's sendPhysicsShapeType /
sendPhysicsGravity / sendPhysicsFriction / sendPhysicsDensity /
sendPhysicsRestitution). The display-side "Show Physics Shape When
Editing" toggle stays in [[viewer-build-display-options]].

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/floater_tools.xml` (L3173-3283),
`indra/newview/llpanelvolume.cpp`.
