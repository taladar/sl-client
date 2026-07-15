---
id: viewer-preferences-audio-tab
title: Preferences — audio tab
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-preferences-ui
blocked_by: [viewer-preferences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The **audio** tab of the preferences floater ([[viewer-preferences-floater]]):
the master and per-bus volumes (ambient, sound effects, UI, media, voice,
streaming) and the mute-on-focus-loss and related audio toggles — each control
bound to the typed settings store through the floater's binding.

The actual audio backend is a separate, out-of-scope concern; this tab owns the
settings surface only.

Reference (Firestorm, read-only): `llfloaterpreference*` (the audio panel).

Builds on: [[viewer-preferences-floater]].
