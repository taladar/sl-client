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

## Parity-audit addendum (2026-08-19)

The parity audit found five more Advanced/World-menu toggles for this
task's cluster. **Set Window Size…** (Advanced ▸ UI/window sizing):
exact window-resolution presets for machinima — we have live UI scale
(`preferences_general.rs`, `SETTING_UI_SCALE`) but no exact-resolution
window sizing; this also touches the capture-resolution presets in
viewer-video-recording. **Mouse Smoothing**: a pointer-smoothing
setting; no equivalent setting exists in our viewer source. **Cheesy
Beacon**: the style toggle for the tracking beacon (the beacon beam
itself is done, `beacons.rs`). **Reset View / Reset Camera Angles /
Zoom In / Zoom Default / Zoom Out**: the reference exposes these camera
shortcuts as menu-visible commands with accelerators; ours exist only
as raw camera input. Finally, World ▸ Show More ▸ **Advanced Menu**
(`UseDebugMenus`): the reference hides the whole Advanced menu behind
this toggle, while our Advanced menu is unconditionally visible — add
the gating toggle.

Add the **Window Size** floater (`floater_window_size.xml`): a small
floater with an exact-resolution presets combo (1024x768 etc.) plus
free-form width/height to set the window to a precise size — used for
machinima and reproducible screenshots.

Parity-audit extensions (merged from the preferences and settings
audits): remove the fly-height limit (`FSRemoveFlyHeightLimit`),
keep flying after teleport (`FSFlyAfterTeleport`), skip the pre-jump
and landing animations (`FSIgnoreFinishAnimation`), don't turn the
avatar around when walking backwards
(`FSDisableTurningAroundWhenWalkingBackwards` — note: our camera module
docs already claim we never turn the avatar; verify before adding a
toggle), crouch-as-toggle (`FSCrouchToggle` — owned by
[[viewer-movement-quickjump-movelock]], listed for completeness), block
left-click sit (`FSBlockClickSit` — owned by
[[viewer-sit-stand-actions]]), disable the teleport/login/logout
progress screens (`FSDisableTeleportScreens`, `FSDisableLoginScreens`,
`FSDisableLogoutScreens`), scroll-wheel exits mouselook
(`FSScrollWheelExitsMouselook`) and show-UI-in-mouselook
(`FSShowInterfaceInMouselook`) — both also clustered in
[[viewer-mouselook-ui-options]], a drag-distance limit twin to the
existing select-distance pair (`LimitDragDistance` /
`MaxDragDistance`), and extended tooltip info + delays
(`FSAdvancedTooltips`, `ToolTipDelay`, per-kind inspector delays) on
top of the hover-tip toggles this task already carries.
