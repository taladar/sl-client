---
id: viewer-mesh-model-upload
title: Mesh / model importer & upload
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The big creator cluster: import a model (COLLADA `.dae` and glTF) and drive the
model-preview floater with the four discrete geometry **LODs** (auto-generate or
per-LOD upload), a **physics** shape, **skin weights / joint** binding for
rigged mesh, material / texture assignment, a streaming-cost + L$ estimate, and
the multi-part mesh-asset upload. Also cover Firestorm's **local mesh** (preview
a mesh from disk in-world before uploading).

**Lean on third-party importers** (`gltf`, `mesh-loader`, or a COLLADA crate)
rather than hand-writing parsers — library selection is a first fleshing-out
step. The LLMesh **encode** side is the inverse of the existing `sl-mesh`
decode.

Reference (Firestorm, read-only): `llfloatermodelpreview`, `llmodelpreview`,
`llfloatermodeluploadbase`, `daeexport`, `fslocalmeshimportgltf`,
`vjlocalmesh*`, `llmeshrepository` (upload side).

Builds on: `sl-mesh` (encode = inverse of the existing decode) and the asset-
upload caps.

Deps: [[viewer-ui-framework]], [[viewer-prim-texture-editing]] (material /
texture assignment overlap).
