---
id: viewer-preferences-camera-move-tab
title: Preferences — camera + move-and-view tab
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-preferences-ui
blocked_by: [viewer-preferences-floater]
refs: [viewer-camera-presets, viewer-autopilot-click-to-walk, viewer-movement-controls-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The **camera** and **move-and-view** tab of the preferences floater
([[viewer-preferences-floater]]): camera follow / orbit distances and angles,
the camera presets, mouse sensitivity and invert, and the movement options (walk
/ run, fly, auto-pilot, single-click / double-click actions) — each control
bound to the typed settings store through the floater's binding.

Reference (Firestorm, read-only): `llfloaterpreference*` (the camera and
move-and-view panels).

Builds on: [[viewer-preferences-floater]].

## Done

New tab module `src/preferences_camera_move.rs` (`TAB_ID = "camera-move"`,
"Camera & movement", after the chat tab), scoped by user decision to **live
controls only** — every row ships with real behaviour, mostly by extracting
the previously hardcoded `camera.rs` / `movement.rs` constants into
registered settings whose defaults reproduce them exactly:

- **Camera**: field of view (`CameraAngle`, applied onto the world camera's
  perspective projection — previously fixed at Bevy's default), camera
  distance scale (`CameraOffsetScale`), smoothing half-life
  (`CameraSmoothingHalfLife`, 0 snaps), maximum camera distance
  (`CameraMaxDistance`), wheel-zoom disable
  (`FSDisableMouseWheelCameraZoom`; alt-drag zoom stays). Sliders carry the
  reference View tab's per-row reset-to-default button.
- **Mouselook**: sensitivity (`MouseSensitivity`, the reference 0–15 scale ×
  0.001 = radians/pixel, so the default 3.0 equals the old hardcoded 0.003),
  invert vertical look (`InvertMouse`, new), show my avatar in mouselook
  (`FirstPersonAvatarVisible`, new — hides the own body-root anchor, which
  carries body / skeleton / world attachments but not HUDs or name tags;
  defaults **on**, today's behaviour, where the reference hides).
- **Movement**: tap-tap-hold-to-run (`AllowTapTapHoldRun`, a new gesture in
  `drive_avatar_controls`, enabled by default like the reference), automatic
  fly on held jump (`AutomaticFly`, gating the existing P31.16 hold-to-take-
  off; auto-land stays unconditional), avatar turn speed (`AvatarTurnRate`
  in honest rad/s, default 3.2 — deliberately not the reference's
  percent-of-default `FSAvatarTurnSpeed` encoding), and the in-world
  double-click action combo over the existing `DoubleClickAction` (No action
  / Teleport; Walk arrives with [[viewer-autopilot-click-to-walk]]).

Mechanism: new `CameraTuning` / `MovementTuning` resources (defaults = the
old constants, so the gallery and store-less tests behave unchanged),
refreshed per frame from the store by the tab plugin (the SpaceNavigator
settings idiom) and read by `orbit_third_person` / `aim_look` /
`position_camera` / `drive_avatar_controls`; the field of view and the
mouselook avatar visibility are applied by guarded-write poll systems.

Deliberately **not** ported (feature absent; a dead row would violate the
live-controls scope): camera constraints / reset-on-TP / edit- and
appearance-camera motion / mouse warp, the mouselook master toggle and
crosshair, scroll-wheel-exits-mouselook, chat-focus key routing (that is
[[viewer-chat-input-world-autostart]]), walk-backwards turning. Camera
presets stay [[viewer-camera-presets]]; single-click / click-to-walk
[[viewer-autopilot-click-to-walk]]; always-run
[[viewer-movement-controls-floater]]; flycam speeds the flycam / spacenav
settings tasks; the keyboard double-tap window is its own constant on
purpose (see [[viewer-consolidate-double-click-interval]]).

Verified by unit tests (aim-delta scaling / pitch-only invert, offset-scale
geometry, zero-half-life snap, double-tap-run latching, automatic-fly gate,
defaults-equal-constants pinning, FOV clamp + write-only-on-change, distinct
Fluent keys) and live on the local grid (settings-file A/B: FOV and camera
distance visibly change, `[camera]` / `[movement]` sections persist).
