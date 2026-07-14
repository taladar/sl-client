---
id: viewer-ui-sound-effects
title: UI sound effects
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [viewer-audio-backend, viewer-ui-framework]
---

Context: [context/viewer.md](../context/viewer.md).

The non-spatial half of sound: the viewer's own feedback sounds, played on a 2-D
bus with no position and no attenuation. The reference has a whole set of them
(the `UISnd*` settings) — button click, alert, invalid operation, money paid /
received, teleport, snapshot shutter, incoming IM and chat, typing, window open
/ close — each individually overridable and mutable, under their own volume
category.

Two concrete hooks already waiting:

- **The typing sound.** `typing.rs` says it in as many words: P31.9 shipped the
  typing *animation* and deliberately left the *sound* out because the viewer
  has no sound playback. This closes that gap.
- **Gesture sound steps.** The gesture runtime in [[viewer-gestures-ui]]
  sequences animation + sound + chat + wait steps; its sound steps play through
  this bus.

Scope: the sound set and its defaults, loading them (shipped assets vs. fetched
from the grid — the reference's UI sounds are asset UUIDs, so they come down the
same `sl-asset` path), the volume category and per-sound mute, the preferences
surface, and the plumbing that lets any UI or notification raise a sound without
reaching into the audio engine directly.

Reference (Firestorm, read-only): the `llui` sound settings and
`LLUI::sSettingGroups` sound lookups, `llgesturemgr` (sound steps).

Builds on: `typing.rs` (the recorded gap) and the notification / UI surfaces.

Deps: [[viewer-audio-backend]] (device + mixer),
[[viewer-ui-framework]] (the events that raise the sounds).
