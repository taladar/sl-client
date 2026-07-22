---
id: viewer-graphics-presets
title: Graphics presets — save / load / pulldown
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-preferences-graphics-tab]
refs: [viewer-quick-preferences]
---

Context: [context/viewer.md](../context/viewer.md).

Named graphics presets: snapshot the whole graphics-settings group
([[viewer-preferences-graphics-tab]] defines it) under a user-chosen name,
load / delete presets, and switch between them from a **status-bar
pulldown** without opening preferences (the "crank everything down for
this club, back up afterwards" flow). Store each preset as a settings-group
overlay file in the account dirs; the active preset name shows in the
pulldown and the graphics tab.

Reference (Firestorm, read-only): `llpresetsmanager`,
`floater_save_pref_preset.xml`, `panel_presets_pulldown.xml`.

Deps: [[viewer-preferences-graphics-tab]] (the settings group being
snapshotted).
