---
id: viewer-collision-messages-chat
title: Collision events to nearby chat
topic: viewer
status: ready
origin: debug-settings/chat-lines survey (2026-07-23)
refs: [viewer-bumps-floater, api-g13]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm can announce agent collisions as chat lines — useful for
combat/roleplay meters and for debugging who keeps bumping you. The
collision events themselves are already received ([[api-g13]],
`MeanCollisionAlert` and friends); this task adds the settings-gated
chat output.

Scope:

- On an agent collision event, emit a viewer-originated
  `ChatSource::System` line into nearby chat naming the collider and
  collision type (`FSCollisionMessagesInChat`).
- Optional report to an arbitrary channel for meters
  (`FSReportCollisionMessages`, `FSReportCollisionMessagesChannel`).
- De-duplicate bursts (the sim can send repeated collisions per contact)
  the way the reference does.

Reference (Firestorm, read-only): the `FSCollisionMessages*` settings and
their consumers in `llviewermessage.cpp` (mean-collision handling).

Builds on: receive-side collision events (done) and the shared
viewer-originated chat-notice emitter ([[viewer-generated-chat-notices]]
introduces it; whichever lands first brings the helper).
