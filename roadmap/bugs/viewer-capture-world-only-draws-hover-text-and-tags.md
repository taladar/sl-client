---
id: viewer-capture-world-only-draws-hover-text-and-tags
title: A world-only capture holds hover text and name tags, which Firestorm draws only with the UI
topic: viewer
status: bugs
origin: sl-crosscheck --scenario scripts (server-fake-grid-scripted-scenario, 2026-11-13)
refs: [viewer-capture-layers, server-fake-grid-scripted-scenario]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`sl-crosscheck --scenario scripts --camera-position 128,121,28
--camera-look-at 128,132,25.5` with the default layers (world only). The
sl-client frames show the props' hover text ("Pay me", "Sit to pose",
"Touch to rez a spark", …) and the own avatar's name tag. The Firestorm
frames of the same scene and camera show neither.

## Cause

Firestorm draws both as `LLHUDObject`s (`LLHUDText`, `LLHUDNameTag`), and
`LLHUDObject::renderAll()` runs only under
`RENDER_DEBUG_FEATURE_UI` (`llviewerdisplay.cpp`, the `render_ui` branch
beside `render_ui_2d()`). A snapshot with `show_ui = false`, and the live
viewer with the UI hidden (Ctrl+Alt+F1), draws neither. Hover text and tags
belong to the UI layer, not the world.

sl-client draws both with the world camera: `name_tag_billboard` (which
hover text reuses, `sl-viewer-world-objects`) gives every tag entity
`RenderLayers` layer 0, so the capture routing of [[viewer-capture-layers]]
sends them with the world to the capture target, whatever `--capture-ui`
says.

## Wanted

- Hover text and name tags are in a capture exactly when the UI is
  (`--capture-ui` / `SL_VIEWER_CAPTURE_UI`), as in Firestorm. A world-only
  frame holds neither, and a frame with the UI holds both.
- They keep their depth behaviour against the world while they are drawn:
  the billboard pulls each tag toward the camera and tests depth, which a
  plain move to the UI camera would lose. One option is to keep them on a
  world-pass layer of their own that the world camera renders unless the run
  is world-only. Keep the probe cameras out of it too
  ([[viewer-name-tags-lost-to-probe-cameras]]).
- Hiding the UI in the live viewer, once the viewer has that toggle, hides
  them as well.
- A test: a world-only capture of a scene with hover text holds no tag
  entity on a layer the capture renders. A `--capture-ui` capture does.
