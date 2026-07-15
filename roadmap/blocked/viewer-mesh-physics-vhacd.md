---
id: viewer-mesh-physics-vhacd
title: Physics "Analyze" via parry3d V-HACD
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-mesh-model-upload
blocked_by: [viewer-mesh-gltf-import]
---

Context: [context/viewer.md](../context/viewer.md).

Compute the model's **physics shape** — the "Analyze" step — with **`parry3d`'s
pure-Rust V-HACD, which we already ship** (via avian3d in the physics stack). So
the heaviest-sounding piece of the importer needs no new dependency and no C++.

`VHACD::decompose` + `compute_convex_hulls` returns one hull per part — exactly
the `physics_convex` `HullList` shape the encoder ([[viewer-mesh-encoder]])
serialises — and parry's own `convex_hull` covers the single bounding hull for
the simplest case. Feed it the imported geometry from
[[viewer-mesh-gltf-import]].

The stock LL viewer used **Havok** here; Firestorm swapped in an open V-HACD,
which is what we mirror. **Match Firestorm's decomposition knobs**: max hulls
(default 8, ≤256), vertices per hull (default 32, ≤256), error tolerance, and
voxel resolution. Pure-Rust CoACD (`CoACD-rs`, WIP) is a future higher-quality
tier if wanted — still no C++.

Reference (Firestorm, read-only): `llconvexdecompositionvhacd.cpp` (the open
V-HACD Firestorm uses in place of Havok).

Builds on: [[viewer-mesh-gltf-import]] and `parry3d` (already in the physics
stack).
