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

Own-avatar animation playback already exists
(`sl-viewer-world-avatar/src/animations.rs`, `locomotion.rs`); this task adds
the step sequencer and the trigger firing.

**Sound steps** (audit 2026-10-15): the audio backend landed
([[viewer-ui-sound-effects]] is done; in-world sounds play in
`sl-viewer-audio/src/world_sounds.rs`), so sound steps are no longer deferred.
Per the measurement below a sound step only sends `Command::TriggerSound` and
lets the grid's echo play it through the existing world-sound path.

Reference (Firestorm, read-only): `llgesturemgr`, `llmultigesture`.

Builds on: `animations.rs` / `locomotion.rs` playback.

## Parity-audit addendum (2026-08-19)

Parity-audit extension: a master enable for gesture-triggered sounds
(`EnableGestureSounds`, sound tab) — play/skip the sound steps when
executing gestures.

## Measured (2026-10-13, [[gridspec-viewer-effects-sounds]])

A gesture's sound step is a `Command::TriggerSound`, and **both grids send it
back to the sender**: play it from that echo, as the reference does, not
locally as well. Second Life sends it to the whole region and OpenSim to
whoever is within 100 m of the stated position; neither passes it on from a
parcel that bars avatar sounds (`any_av_sounds`), where the gesture is then
silent for its own avatar too. Tables in `book/src/gridspec/avatars.md`
§ Effects and sounds.
