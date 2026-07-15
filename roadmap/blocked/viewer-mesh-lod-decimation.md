---
id: viewer-mesh-lod-decimation
title: LOD decimation via meshoptimizer
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-mesh-model-upload
blocked_by: [viewer-mesh-gltf-import]
---

Context: [context/viewer.md](../context/viewer.md).

Generate the four discrete geometry **LODs** for an imported model
([[viewer-mesh-gltf-import]]) with the **`meshopt` crate** — and this is a place
we can *beat* the reference rather than merely match it.

This is where SL's own "avoid platform-specific C++" scars are: mesh upload was
disabled on the Linux (and 64-bit) viewer for years because its LOD generator,
**GLOD**, was not redistributable / not buildable there. Linden Lab fixed it by
replacing GLOD with **meshoptimizer**. Our design starts on the far side of that
fix.

`meshopt` is the one C++ dependency in the whole importer (verified not already
in our tree — Bevy's meshlet feature would link it, but the viewer does not
enable that), and it is the acceptable kind: a small, self-contained,
dependency-free, MIT-licensed C++ TU compiled via `cc` (no cmake, no system
libs). `meshopt_simplify` is a proper QEM (Garland–Heckbert) edge-collapse — not
a speed hack. It also does the geometry prep the encoder wants
(`generate_vertex_remap` weld/index, `optimize_vertex_cache`,
`optimize_vertex_fetch`).

- Use **`simplifyWithAttributes`** with **UV weighting** and **seam locking**
  for the LODs. The known knock on meshopt — Firestorm keeps GLOD as a
  "reliable" toggle because meshopt is worse ~2/3 of the time — is specifically
  **UV-seam / attribute damage**, and the reference viewers *under-use* the
  attribute-aware path. Using it properly is how we close most of the GLOD gap
  while staying in one algorithm.
- The topology-ignoring **`simplifySloppy`** is a separate call, used for the
  **farthest LOD only**.

The "avoid FFI to dodge crashes" argument does **not** apply here: the SL Linux
glTF-upload crashes are in the import / validation path, not the decimator, and
meshoptimizer is among the most battle-tested C libraries in graphics.

Pure-Rust alternatives were surveyed and are watch-list, not ready:
**baby_shark** (0.3.12, real boundary-aware QEM, MIT) is healthiest, but its
collapse cost is **geometry-only — no UV / attribute term**, so it would regress
the exact axis SL creators care about; keep it as a candidate for an optional
"reliable" second
pass on *untextured* meshes only, and a real contender the day it grows
attribute-aware quadrics. The pure-Rust `meshopt-rs` port's `simplify` is
unreleased and abandoned since 2022.

Reference (Firestorm, read-only): `llmodelpreview.cpp` (`genMeshOptimizerLODs`,
the LOD targets).

Builds on: [[viewer-mesh-gltf-import]] and the `meshopt` crate.
