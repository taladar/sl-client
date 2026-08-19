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

## Parity-audit addendum (2026-08-19)

The parity audit adds the smaller Develop-menu console/readout
surfaces: the **Notifications console**, the **Camera / Wind / FOV
mini-consoles** (Develop ▸ Consoles, menu_viewer.xml L3880–4047), and
the Show Info corner readouts (L4050–4152): **Show Time, Show Render
Info, Show Matrices, Show Color Under Cursor, Show Memory, Show
Texture Info, Show Upload Transaction** — small on-screen overlay
readouts toggled per item. (Show Updates to Objects is already covered
by the render-metadata task; VRAM-per-object totals fold into
viewer-statistics-floater.)

Add the **notifications console** (`floater_notifications_console.xml`)
— the dev floater that lists the notification catalogue and lets you
browse/fire individual notification entries for testing, a natural fit
next to the other debug consoles this task builds.
