---
id: viewer-camera-behavior-options
title: Camera-feel behaviour toggles
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-camera-third-person-orbit, viewer-camera-mouselook,
  viewer-preferences-camera-move-tab, viewer-derender-blacklist]
---

Context: [context/viewer.md](../context/viewer.md).

Small reference behaviour switches around an already-implemented camera —
the module docs of `preferences_camera_move.rs` list most of these as
deliberately unported because no backing feature exists yet
([[viewer-preferences-camera-move-tab]] is done, this task is the missing
backing behaviours plus their toggles):

- Auto-reset the camera to the rear view on avatar movement
  (`FSResetCameraOnMovement`) and on teleport (`FSResetCameraOnTP`). The
  reset capability itself exists (`camera.rs`, Escape) — only the
  auto-triggers are missing.
- Mouselook mouse smoothing (`MouseSmooth`) — we have sensitivity and
  invert only (`preferences_camera_move.rs`).
- Edit-mode / appearance-mode automatic camera motion
  (`EditCameraMovement`, `AppearanceCameraMovement`,
  `FSAppearanceShowHints`): zoom the camera to the selection or the
  avatar when entering the edit / appearance tools.
- Turn-avatar-toward-camera on reset view (`ResetViewTurnsAvatar`) and
  turn-avatar-to-selected-object (`FSTurnAvatarToSelectedObject`).
- Mouse-warp mode (`MouseWarpMode`): wrap the pointer at screen edges
  during camera drags.
- Fly-after-teleport (`FSFlyAfterTeleport`): keep/resume flight on
  arrival.
- Re-render temp-derendered objects after teleport
  (`FSTempDerenderUntilTeleport`) — pairs with the derender machinery
  from [[viewer-derender-blacklist]].

For completeness of record: `ClickOnAvatarKeepsCamera` needs no work —
clicking an avatar never refocuses our camera in the first place, so the
reference toggle disables a behaviour we never had. Each implemented
behaviour goes behind a setting for the camera/move preferences tab to
surface.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_preferences_move.xml`,
`indra/newview/skins/default/xui/en/floater_phototools.xml` (Cam tab),
`indra/newview/llagentcamera.cpp`, `indra/newview/fsfloaterphototools.cpp`.
