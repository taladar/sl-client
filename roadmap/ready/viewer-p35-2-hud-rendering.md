---
id: viewer-p35-2
title: HUD rendering
topic: viewer
status: ready
origin: VIEWER_ROADMAP.md — Phase 35 — HUD attachments
blocked_by: [viewer-p35-1]
---

Context: [context/viewer.md](../context/viewer.md).

**P35.2. HUD rendering.** Render HUD-attached prims/mesh on a HUD camera /
render layer anchored per the HUD attachment-point screen layout (orthographic
/ screen-relative), reusing the existing prim/mesh geometry+texture build.
Verify a simple HUD renders fixed to the screen on aditi.

Blocked on [[viewer-p35-1]]: renders the attachments that step detects and
routes to the HUD layer.
