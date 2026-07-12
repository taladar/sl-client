---
id: viewer-p2-3
title: Seamless patches + multi-region terrain
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 2 — Terrain
---

Context: [context/viewer.md](../context/viewer.md).

**P2.3. Seamless patches + multi-region terrain.** Two fixes discovered
when rendering live: (a) each patch mesh now spans its full 16 m edge —
`(size+1)²` vertices sampling the far edge from the north/east/NE neighbour
patches (Firestorm `LLSurfacePatch` stitching) — closing the 1 m gaps that
made P2.1/P2.2 terrain look fragmented; (b) terrain now streams and renders
across the agent's region **and** its neighbour child circuits: patches are
keyed by `(region_handle, patch_x, patch_y)`, each region has its own
composition + splat material, and patches are placed at a global offset from a
moving scene origin that follows the root region (recenter shifts the
fly-camera by the same delta so `f32` precision holds far from the login
region while the world stays continuous across border crossings). The draw
distance was raised to 512 m so the sim announces neighbours. Required one
`sl-proto` fix: a neighbour's `RegionHandshake` on a child circuit now also
emits `RegionInfoHandshake` (previously dropped), so neighbour terrain gets
its own textures rather than the placeholder.
