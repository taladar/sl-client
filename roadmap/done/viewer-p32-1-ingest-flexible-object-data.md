---
id: viewer-p32-1
title: Ingest flexible-object data
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 32 — Flexi prims
---

Context: [context/viewer.md](../context/viewer.md).

**P32.1. Ingest flexible-object data.** The `LLFlexibleObjectData` extra
params (softness, gravity, drag, wind, tension, force). The Bevy-free wire
decode already existed in `sl-proto` (`decode_flexible` → [`FlexibleData`] on
`Object::extra.flexible` — the four packed tension / drag / gravity / wind
bytes, the two simulate-LOD "softness" bits stashed in their high bits, and
the trailing user-force vector; `FlexibleData` already re-exported from both
runtime crates), so the net-new work was the **viewer-side ingest**, mirroring
the P25.1 light / P30.1 particle ingest exactly: a new
`sl-client-bevy-viewer::flexi` module with an `ObjectFlexi` component carrying
the decoded block, a `flexi_from_object` lift, and an `apply_flexi` reconcile
that `apply_object` calls on both the spawn and update paths (beside
`apply_light` / `apply_particles`) so a prim toggled flexi on / off between
updates is tracked. Unlike the particle system there is no null / sentinel
form to reject — the reference viewer's `LLVOVolume::isFlexible` treats a
prim as flexi exactly when the block is present (`getFlexibleObjectData()`
non-null) —
so the lift is a straight `Option`: present → attach, absent → remove. The
component rides the **object entity** (its world transform) the way
`LLVolumeImplFlexible` anchors its chain at the prim root, ready for the P32.2
chain simulation. Flexi is mutually exclusive with server physics (the
reference forces a flexi prim phantom + non-physical in `setIsFlexible`), so a
flexi prim never also carries the P31.2 physics-body marker, and the whole
deformation is client-side (the simulator sends no per-frame flexi state).
**Live-verified on OpenSim:** a hand-built `slclient-flexi.oar` (a tall thin
flexi cylinder in the Default Region) is ingested with exactly its set params
(softness 2, tension 1.0, drag 2.0, gravity 0.3, wind 0.0, force (0,0,−0.5));
clean build/clippy and two new unit tests over the present-vs-absent lift.
