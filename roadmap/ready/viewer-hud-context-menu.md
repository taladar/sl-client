---
id: viewer-hud-context-menu
title: HUD context / pie menu entries
topic: viewer
status: ready
origin: gap noticed reviewing the UI cluster (2026-07)
blocked_by: [viewer-ui-radial-menu]
refs: [viewer-ui-context-menu, viewer-object-context-menu]
---

Context: [context/viewer.md](../context/viewer.md).

The **entries** offered when a **HUD attachment** is the pick target: Touch,
Edit, Detach (to inventory), and the HUD-specific actions — distinct from the
in-world object menu ([[viewer-object-context-menu]]) because a HUD is
screen-space and already attached. HUD picking / clicking already exists; this
task is the menu entries and their dispatch.

Rendered by either the radial ([[viewer-ui-radial-menu]]) or line
([[viewer-ui-context-menu]]) widget.

Reference (Firestorm, read-only): `menu_hud.xml`, `llhudview`.
