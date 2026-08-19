---
id: viewer-fullscreen-mode
title: Fullscreen / borderless window mode
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-input-action-map, viewer-ui-floater-persist-geometry,
  viewer-preferences-graphics-tab]
---

Context: [context/viewer.md](../context/viewer.md).

The reference has a `FullScreen` setting — a checkbox on the graphics
preferences tab — that starts the viewer fullscreen and switches the
window into (borderless) fullscreen and back at runtime. Our window is
created plain windowed with no mode control anywhere: the startup code in
`sl-client-bevy-viewer/src/lib.rs` builds a default Bevy `Window` setting
only title, name and cursor options, and nothing later touches
`WindowMode`.

Scope: a persisted window-mode setting (windowed / borderless fullscreen;
winit `WindowMode` via Bevy) applied at startup and toggleable at runtime
through the conventional F11 / Alt+Enter binding in the input action map
([[viewer-input-action-map]]), persisting alongside the existing
floater-geometry persistence ([[viewer-ui-floater-persist-geometry]] is
the pattern for remembering window state). Monitor selection can stay
default. The graphics tab gains the checkbox
([[viewer-preferences-graphics-tab]] is done; this is a new row).

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_preferences_graphics1.xml`
(FullScreen), `indra/newview/llviewerwindow.cpp`.
