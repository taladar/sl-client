---
id: viewer-capture-world-only-draws-hover-text-and-tags
title: A world-only capture holds hover text and name tags, which Firestorm draws only with the UI
topic: viewer
status: done
origin: sl-crosscheck --scenario scripts (server-fake-grid-scripted-scenario, 2026-11-13)
refs: [viewer-capture-layers, server-fake-grid-scripted-scenario, viewer-world-text-in-the-overlay-pass, viewer-snapshot-floater]
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

## Resolution (2026-11-14)

The world-anchored UI has a render layer of its own,
`probe_layers::WORLD_UI_LAYER` (9). It covers hover text and name tags
(`tag_render_layers()` is still the shared seam and now returns that layer),
the tracking beacon's beam, the debug beacons and the parcel borders. The
last three were added because the reference gates them on the same flag:
`render_hud_elements` draws the tracker beacon and property lines only under
`RENDER_DEBUG_FEATURE_UI`, and `render_ui_3d` does the same for
`renderObjectBeacons`. The beacon's label and arrow are `bevy_ui` nodes, so
they already left with the UI.

They are still drawn by the world camera, so they keep their depth behaviour
against the scene. The main camera's layers are
`main_camera_render_layers(world_ui)`, and `WorldUiVisibility` (world-api)
decides `world_ui`. It is a set of hiders, so one hider letting go does not
show the layer while another still holds it. `WorldUiLayerPlugin`
(world-scene) applies it to the `ViewerCamera` every frame in `PostUpdate`
before visibility is checked, so a camera spawned later is corrected too.
Neither sun and no probe camera is on the layer.

Who hides it:

- the capture harness, for the whole run, unless `--capture-ui` is set. This
  works for both capture targets, the pinned image and the headless window;
- the snapshot floater with **Include UI** off, for the frames of the shot.
  The reference's snapshot without the UI drops its `LLHUDObject`s the same
  way. What a shot hides and restores is now one `ShotLayers` system
  parameter.

The live viewer has no hide-UI toggle yet. When it gets one, it adds a hider
to `WorldUiVisibility`.

Verified with `sl-crosscheck --scenario scripts --only sl-client
--camera-position 128,121,28 --camera-look-at 128,132,25.5 --frames 3`. The
world-only frames show the props and the avatar with no hover text and no
tag. The same pose with `--capture-ui` shows "Pay me", "Sit to pose", "Touch
to rez a spark" and the "Test User" tag. An earlier `--capture-ui` run had
drifted off the fixed pose with the flycam active. A rerun held the pose, so
this matches the known hazard of a capture window receiving live input, not
this change.

Tests: `the_world_ui_is_drawn_only_by_the_main_camera_and_only_with_the_ui`
(probe_layers), `the_world_ui_leaves_the_view_while_any_hider_holds_it` and
`a_camera_spawned_while_the_ui_is_held_off_does_not_draw_it` (viewer_camera),
`hover_text_and_name_tags_are_in_a_capture_exactly_with_the_ui` (screenshot:
world-only and HUD+gizmos leave them out, UI keeps them), and
`a_shot_without_the_ui_takes_the_world_ui_with_it` (snapshot_floater).
