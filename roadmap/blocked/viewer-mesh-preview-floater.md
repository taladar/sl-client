---
id: viewer-mesh-preview-floater
title: Model-preview floater
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-mesh-model-upload
blocked_by: [viewer-ui-widget-scaffold, viewer-mesh-gltf-import, viewer-mesh-lod-decimation, viewer-mesh-physics-vhacd, viewer-mesh-upload-sequence, viewer-prim-texture-editing]
---

Context: [context/viewer.md](../context/viewer.md).

The **model-preview floater** that ties the whole creator cluster together:
import a model, then drive a preview with the four discrete geometry **LODs**
([[viewer-mesh-lod-decimation]]), the **physics** shape
([[viewer-mesh-physics-vhacd]]), **skin / joint** binding for rigged mesh
([[viewer-mesh-gltf-import]]), **material assignment** (overlapping the
texture-editing tool, [[viewer-prim-texture-editing]]), and a **land-impact /
L$ estimate** display ([[viewer-mesh-cost-estimate]] for the client-side land
impact, the step-1 fee round-trip in [[viewer-mesh-upload-sequence]] for the
L$). The upload itself is [[viewer-mesh-upload-sequence]].

Also implement Firestorm's **local mesh**: preview a mesh from disk in-world,
before uploading, so a creator can iterate on geometry against the live scene
without spending an upload. This reuses the same import + LOD + preview path,
rendering the imported model as a temporary in-world object.

The floater is hosted on the UI widget scaffold ([[viewer-ui-widget-scaffold]]).

Reference (Firestorm, read-only): `llfloatermodelpreview`, `llmodelpreview.cpp`
(the preview + LOD targets), and `vjlocalmesh*` (Firestorm's local-mesh
in-world preview).

Builds on: [[viewer-ui-widget-scaffold]], [[viewer-mesh-gltf-import]],
[[viewer-mesh-lod-decimation]], [[viewer-mesh-physics-vhacd]],
[[viewer-mesh-upload-sequence]] and [[viewer-prim-texture-editing]] (material /
texture assignment overlap).
