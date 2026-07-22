---
id: viewer-lookat-faithful
title: Faithful look-at (LLHUDEffectLookAt)
topic: viewer
status: ready
origin: split from the camera-system pass (the debug camera-stare stand-in was removed)
blocked_by: [viewer-camera-third-person-orbit]
---

Context: [context/viewer.md](../context/viewer.md).

A faithful reimplementation of the reference's agent look-at
(`LLHUDEffectLookAt` + `LLAgentCamera::setLookAt`), replacing the minimal
camera-mode stand-in the camera-system pass left in `crate::look_at`
([`update_own_look_at_target`]): mouselook looks along the camera forward;
third person / flycam send nothing. That stand-in exists only to kill the old
debug behaviour (the avatar stared *at* the camera, which fed back into the
head-focused camera and vibrated the view).

The faithful version, per the reference:

- **The look-at target types** and their priorities — `MOUSELOOK`, `FREELOOK`,
  `FOCUS` (an alt-clicked / focused object, with the per-object offset math of
  `setFocusGlobal`), `SELECT` (editing / grabbing an object), `CONVERSATION`,
  `RESPOND`, `AUTO_LISTEN`, `IDLE` (the random idle glances), `CLEAR`.
- **Own-avatar handling by entity** — alt-clicking your *own* avatar must not
  drive a look-at (you don't look at yourself), while a look-at target *on
  another* nearby avatar (a cuddle partner) is valid. Detect the picked entity's
  avatar, not by proximity.
- **The enabling settings** and the **animation-priority** interactions (a
  higher-priority played animation overrides the look-at head/eye pose).
- **A debug gizmo** drawing every avatar's current look-at target as a small 3D
  marker, like the reference's "Show Look At"
  (`LLHUDEffectLookAt::sDebugLookAt`) — and its Develop-menu twin **"Show
  Point At"** (`LLHUDEffectPointAt` targets), so both halves of the pair
  land together.

Reference (Firestorm, read-only): `indra/newview/llhudeffectlookat.cpp/h`,
`indra/newview/llagentcamera.cpp` (`setLookAt` / `setFocusGlobal`),
`indra/newview/app_settings/settings.xml` (the `LookAt*` settings).
