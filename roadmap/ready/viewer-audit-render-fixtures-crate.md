---
id: viewer-audit-render-fixtures-crate
title: 3297 lines of test fixtures ship in the production scene crate
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 3
refs: [viewer-audit-binary-module-extraction]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-scene/src/render_scene.rs` is 3297 lines of **test fixtures** —
procedural prims, sculpts, meshes, skeletons, demo scenes — shipped as
`pub mod render_scene` with no `cfg` and no feature gate.

It is well-argued in its own module docs and genuinely valuable; it just belongs
in a `sl-viewer-render-fixtures` crate. Today it drags `sl-terrain`, `Bytes` and
a committed `.llm` into every consumer of the scene layer.

Same shape as the gallery modules in
[[viewer-audit-binary-module-extraction]] — harness code compiled into a library
22 crates depend on.
