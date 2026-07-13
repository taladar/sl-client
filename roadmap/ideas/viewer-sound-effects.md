---
id: viewer-sound-effects
title: Spatial sound-effects engine
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The other half of audio: 3-D positional sound effects — `llTriggerSound`,
attached / looped object sounds, collision sounds, gesture sounds, and UI
sounds. Fetch + decode the sound asset, spatialise it against the listener
(avatar/camera), attenuate by distance, and mix. Distinct from the streaming
player (this stub) and it shares the same audio backend.

The sound **receive** protocol is already done (`protocol-22`); this is the
decode + spatialise + playback engine.

Reference (Firestorm, read-only): `llaudio/llaudioengine_*`, `lllistener_*`,
`lldeferredsounds`, `llaudiodecodemgr`.

Builds on: `protocol-22` sound-receive and `sl-asset` for asset fetch.
Supersedes the MVP "no sound" non-goal.

Deps: [[viewer-streaming-audio]] (shared audio backend).
