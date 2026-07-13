---
id: viewer-preferences-ui
title: Preferences / settings UI
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-framework]
---

Context: [context/viewer.md](../context/viewer.md).

The settings floater — graphics, audio, chat / IM, privacy, camera, move-and-
view, and keybindings — backed by a persistent, typed settings store with
sensible defaults and per-account overrides. This is where the input system's
key rebinding UI lives.

Reference (Firestorm, read-only): `llfloaterpreference*`, `llviewercontrol`
(settings backend), `fspanelprefs`, `llfloatersettingsdebug` (raw debug
settings editor).

Deps: [[viewer-ui-framework]].
