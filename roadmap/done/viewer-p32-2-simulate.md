---
id: viewer-p32-2
title: Simulate
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 32 — Flexi prims
---

Context: [context/viewer.md](../context/viewer.md).

**P32.2. Simulate.** Port the reference spring / chain deformation of
the prim path over time (`LLVolumeImplFlexible` on `LLVOVolume`), built on
the Phase 31 avian primitives where practical, deforming / re-tessellating
the flexi geometry each frame. **Done:** a new pure **`sl-prim::flexi`**
module (`FlexiChain` + `FlexiAttributes`) faithfully ports
`setAttributesOfAllSections` (chain init) and `doFlexibleUpdate` (per-frame
integration): a world-space (Z-up metre) node chain with gravity, user force,
chain tension toward the parent, and inertia, an angle-clamped shortest-arc
bend per section, and half-bend propagation to the parent — the same
distance-constrained, angle-clamped solver the reference runs. It is **not**
built on avian: the flexi chain is a bespoke constraint solver, not rigid
bodies, so a faithful port is the fit the "where practical" wording
anticipates (avian's dynamic bodies stay reserved for the rigid-body client
motion of P31 / P34). `FlexiChain::path` reads the deformed chain out as an
extrusion path in **full-size metre** geometry (the prim scale baked into the
profile *before* the section rotation), fed through a new
**`sl_prim::tessellate_with_path`** (factored out of `tessellate` — the sweep
over a caller-supplied path). The viewer's `flexi` module owns the ECS glue: a
`FlexiSimState` seeded at build from the chain's rest path, and a
`simulate_flexi` system that each frame reads the prim's live world pose from
its `GlobalTransform` (inverting the single SL↔Bevy basis change), steps the
chain, re-sweeps, and overwrites the face meshes in place (faces marked
`NoFrustumCulling`, like particles). A flexi prim is taken off the pixel-area
LOD re-tessellation path and given an **identity geometry holder** (like
grass) so its already-metre geometry is not re-scaled — the load-bearing fix
found live: a unit-local mesh scaled by a very non-uniform holder *after* the
bend shears the cross-section catastrophically (a `0.3×0.3×4 m` flexi cylinder
ballooned into a slab once it drooped), which the metre bake avoids. The
`ShapeFingerprint` gained the flexi softness so a softness / toggle change
rebuilds (and re-seeds) the chain, while the live tension / gravity /
user-force params drive the sim each frame with no rebuild. UVs are set once
at build and not re-projected as the prim bends (a planar-texgen face's
projection is frozen at rest; ordinary per-face texgen UVs are parametric and
stay correct); wind is not simulated (no region wind field). **Live-verified
on OpenSim** with a new horizontal `slclient-flexi-h.oar` (a `0.3×0.3×4 m`
flexi cylinder laid on its side, gravity 3, softness 3): it renders as a thin
rod drooping into a smooth downward arc, at correct size. Clean build / clippy
and five new unit tests (three in `sl-prim` over the chain, plus the existing
ingest pair updated for the scale-carrying component).
