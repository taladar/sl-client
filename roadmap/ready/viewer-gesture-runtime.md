---
id: viewer-gesture-runtime
title: Gesture runtime — step sequencing + /-command triggers
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-gestures-ui
blocked_by: [viewer-chat-channel-and-commands]
refs: [viewer-ui-sound-effects]
---

Context: [context/viewer.md](../context/viewer.md).

The multi-step **gesture runtime**: sequence a gesture's **animation**,
**chat**, and **wait** steps (wait-for-time and wait-for-animation), and fire a
gesture
from its **`/`-command chat trigger** — which needs the chat channel / command
dispatch ([[viewer-chat-channel-and-commands]]) so a typed `/trigger` is routed
to the gesture instead of being sent as chat.

Own-avatar animation playback already exists (`animations.rs`,
`locomotion.rs`); this task adds the step sequencer and the trigger firing.

**Sound steps are deferred**: they need the audio backend / UI sound bus
([[viewer-ui-sound-effects]]), which is out of scope until that research lands.
Sequence a sound step as a no-op (or a logged placeholder) for now and wire it
to the sound bus as a follow-up.

Reference (Firestorm, read-only): `llgesturemgr`, `llmultigesture`.

Builds on: `animations.rs` / `locomotion.rs` playback.
