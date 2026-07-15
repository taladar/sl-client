---
id: viewer-gestures-ui
title: Gesture management & trigger UI
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-sound-effects]
---

Context: [context/viewer.md](../context/viewer.md).

The gesture surface: an active-gestures list, chat-trigger (`/`-command) firing,
a gesture editor / preview, and driving the multi-step gesture runtime that
sequences animation + sound + chat + wait steps.

Own-avatar animation playback already exists (`animations.rs`,
`locomotion.rs`); this stub adds the gesture runtime (step sequencing) and its
management UI, and plays its sound steps through the UI sound bus
([[viewer-ui-sound-effects]]).

Reference (Firestorm, read-only): `llgesturemgr`, `llfloatergesture`,
`llpreviewgesture`, `llmultigesture`.

Builds on: `animations.rs` / `locomotion.rs` playback.

Deps: [[viewer-ui-widget-scaffold]], [[viewer-ui-sound-effects]] (sound steps).
