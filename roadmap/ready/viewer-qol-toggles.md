---
id: viewer-qol-toggles
title: Advanced-menu quality-of-life toggles
topic: viewer
status: ready
origin: Advanced/Develop menu survey (2026-07-22)
blocked_by: [viewer-input-action-map, viewer-ui-settings-store]
refs: [viewer-movement-controls-floater, viewer-camera-third-person-orbit]
---

Context: [context/viewer.md](../context/viewer.md).

The small Advanced-menu toggles power users reach for, each a
settings-store-backed switch consumed by its owning system (this task wires
menu + keybind + setting; the consuming system change is usually a line or
two):

- **Always Run** (Ctrl+R) — the `SetAlwaysRun` wire toggle (protocol done
  in `idiomatic-p3-02`) + run-by-default movement.
- **Fly override** — allow fly on no-fly parcels where the sim tolerates
  it (FS `FSAlwaysFly`).
- **Limit select distance** — stop selection rays at the reference's
  distance cap (off = build from afar).
- **Disable camera constraints** — ignore the sim camera constraint
  volumes ([[viewer-camera-third-person-orbit]] honours them today).
- **Release keys** — drop taken script controls (the permission registry's
  revoke; menu surface for it).
- **Look at last chatter** (Ctrl+\) — snap camera focus to the most recent
  nearby speaker.
- **Mouselook crosshairs** show/hide; **hover-tips** master + per-kind
  toggles (land, all objects).
- **Hide all UI** (`View.ToggleUI`) — blank the whole interface for
  screenshots/machinima; and **Show HUD Attachments**
  (`View.ShowHUDAttachments`) — temporarily hide worn HUDs (main-menu
  survey 2026-07-23).

Reference (Firestorm, read-only): `menu_viewer.xml` (Advanced), the named
settings (`FSAlwaysFly`, `LimitSelectDistance`,
`DisableCameraConstraints`, `ShowCrosshairs`).

Builds on: the input action map, settings store, and each owning system.
