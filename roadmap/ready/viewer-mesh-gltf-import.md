---
id: viewer-mesh-gltf-import
title: glTF import into an intermediate SL model
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-mesh-model-upload
---

Context: [context/viewer.md](../context/viewer.md).

Import a model with the pure-Rust **`gltf` crate** (MIT/Apache) into an
intermediate SL model type that the encoder ([[viewer-mesh-encoder]]), LOD
decimation, physics and preview steps all consume. The `gltf` crate covers
everything the SL rig needs — multiple primitives (submeshes), material refs,
morph targets, GLB, and full skinning (`Skin::joints` /
`inverse_bind_matrices`, per-primitive `read_joints` / `read_weights`, the node
hierarchy). What is left is **our math**, not a crate gap:

- **Coordinate conversion**: glTF is Y-up right-handed, SL is Z-up
  right-handed — convert positions, normals and the node transforms into SL
  space as they are read.
- **Joint mapping**: map each glTF joint node **name** onto the fixed SL
  skeleton (the `avatar_skeleton.xml` bones the base body / rigged mesh path
  already know), so rigged submeshes bind to real SL joints.
- **Bind-shape matrix**: synthesise SL's `bind_shape_matrix` from the node
  transforms (glTF carries per-joint inverse-bind matrices but not SL's single
  bind-shape).
- **Influence clamp**: clamp / renormalise skin weights to ≤4 influences per
  vertex (SL's per-vertex weight budget), dropping the smallest influences and
  renormalising the survivors.

**COLLADA is deliberately out of the required set.** Blender has dropped its
`.dae` exporter and SL authoring has moved to glTF; Firestorm's own glTF
importer converts to the identical LLMesh format it uses for COLLADA, so
glTF-first costs nothing on the encode side. Do **not** pull in `russimp` /
assimp for `.dae` — a heavy C++ dependency, archived and unmaintained in
late 2025. If `.dae` is ever wanted, add it later as a separate optional
importer over a pure-Rust crate (`dae-parser` reads *and* writes) — never as a
reason to take a C++ toolchain.

Reference (Firestorm, read-only): `llmodel.cpp`, `llmodelpreview.cpp` (the
import path), `fslocalmeshimportgltf` (Firestorm's own glTF-to-LLMesh path).

Builds on: the pure-Rust `gltf` crate and the SL skeleton the avatar crates
already parse.
