---
id: viewer-about-floater
title: About floater — version, system info, credits, licenses
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The About window: viewer name + version (from the crate version / git
describe at build time), the connected grid and region + its simulator
version (`RegionHandshake` / login response data already held), runtime
system info (OS, GPU/renderer as reported by wgpu, CPU, memory), all
**copyable to the clipboard as one block** — the support workflow the
reference's "Copy to clipboard" exists for — plus credits and the
third-party license texts (generate from `cargo about` or similar at build
time rather than hand-maintaining).

Reference (Firestorm, read-only): `llfloaterabout`, `floater_about.xml`.

Builds on: session/region state and build-time metadata.
