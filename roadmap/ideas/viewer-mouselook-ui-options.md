---
id: viewer-mouselook-ui-options
title: Mouselook UI & behaviour options
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-camera-mouselook, viewer-qol-toggles, viewer-mouselook-combat]
---

Context: [context/viewer.md](../context/viewer.md).

Our mouselook camera (done [[viewer-camera-mouselook]]) hides all UI
unconditionally on entry. The reference makes each part of that a
choice: a mouselook master enable (`EnableMouselook`), show the UI
while in mouselook (`FSShowInterfaceInMouselook`), keep the
conversations window and radar open on entering mouselook
(`FSShowConvoAndRadarInML`), allow right-click context menus in
mouselook (`FSEnableRightclickMenuInMouselook`), scroll-wheel exits
mouselook (`FSScrollWheelExitsMouselook`), and a first-entry
instruction overlay (`FSShowMouselookInstructions`).

Crosshair display is already covered by [[viewer-qol-toggles]], and the
combat-oriented mouselook features (IFF, FOV overrides) by
[[viewer-mouselook-combat]] — this task is the UI-visibility and
enter/exit behaviour layer.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_preferences_move.xml`,
`indra/newview/llagentcamera.cpp`,
`indra/newview/app_settings/settings.xml` (named settings).
