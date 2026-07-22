---
id: viewer-debug-consoles
title: Debug consoles — texture, debug text, scene stats
topic: viewer
status: ready
origin: Advanced/Develop menu survey (2026-07-22)
refs: [viewer-statistics-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The keyboard-summoned debug consoles (translucent full-width text overlays,
as the reference's Ctrl+Shift+3/4 family), building on the existing
pipeline-status overlay (`diagnostics.rs`):

- **Texture console**: live fetch/decode table — in-flight requests with
  priority / discard / state, cache hit rates, decode queue depth, memory
  by category; the view into `sl-asset-sched` the "why is that texture
  blurry" question needs.
- **Debug console**: the viewer's own log stream (tracing subscriber tail)
  on screen, with level filter.
- **Scene statistics / scene-loading monitor**: object counts by state
  (pending mesh, pending texture, complete), patch/terrain status —
  effectively a scene-completeness view of the pipeline-status API.
- **Info dumps to chat/log**: region info, caps URLs, group info — the
  reference's "dump to console" utilities (data all held; just formatters).

Reference (Firestorm, read-only): `llconsole`, `lltextureview`
(texture console), `llfloaterstats` siblings, `menu_viewer.xml`
(Develop → Consoles).

Builds on: `diagnostics.rs` overlay + the pipeline-status API.
