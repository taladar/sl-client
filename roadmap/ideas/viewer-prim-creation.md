---
id: viewer-prim-creation
title: Prim creation
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-object-selection, viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

Create new in-world prims: a create / build tool mode, pick a base prim type,
and rez a default prim at a ray-cast build point on a surface. This is the entry
point to the build workflow.

Reference (Firestorm, read-only): `lltoolplacer`, `lltoolcomp` (create); the
`ObjectAdd` message.

Builds on: `objects.rs` lifecycle and `sl-prim` tessellation.

Deps: [[viewer-object-selection]], [[viewer-ui-widget-scaffold]].
