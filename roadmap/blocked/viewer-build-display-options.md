---
id: viewer-build-display-options
title: Build-mode display/overlay toggles
topic: viewer
status: blocked
origin: main-menu survey (2026-07-23)
blocked_by: [viewer-object-selection-core]
refs: [viewer-render-type-toggles, viewer-highlight-transparent]
---

Context: [context/viewer.md](../context/viewer.md).

Edit-mode visualisation switches from Build ▸ Options:

- Show Physics Shape When Editing; Show Reflection Probe Volumes; Show
  Light Radius for Selection
- Show Selection Outlines; Show Hidden Selection (silhouette through
  walls); Show Selection Beam
- Selection level of detail (Default/High/Medium/Low/Lowest — force a
  LOD on selected objects)
- No Post-processing while editing; Show Advanced Permissions (full
  permission masks in the edit floater)

Scope: each toggle as a setting + menu entry, consumed by the renderer
(overlay meshes for physics shapes/probe volumes/light radii, selection
outline/silhouette/beam passes, LOD override on the selection set, a
post-process bypass) and the edit floater (advanced permissions).

Reference (Firestorm, read-only): `menu_viewer.xml` Build ▸ Options
(~L2084-2378), the corresponding debug settings and render-pass hooks.

Builds on: the selection-set core (blocked task); overlay rendering
shares machinery with [[viewer-render-type-toggles]] and
[[viewer-highlight-transparent]].
