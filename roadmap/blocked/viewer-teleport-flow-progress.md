---
id: viewer-teleport-flow-progress
title: Teleport flow — progress screen & arrival
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-teleport-flow
blocked_by: [viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

The user-facing teleport experience: **initiate** a teleport (from the map, a
landmark, a SLURL, or "teleport home"), the teleport **progress** screen with
its state messages and cancel, and the **arrival** hand-off when the destination
region comes up.

The teleport **protocol** is already done and tested (see the Phase-12 teleport
conformance cases — local, cross-region, failed, and offer / accept); this task
is the viewer flow + progress UI on top.

Note: the incoming teleport-**offer / lure** dialog is a case of the
notifications system and lives in [[viewer-dialog-offers-invites]], not here;
this task owns the progress UX only.

Reference (Firestorm, read-only): `llagent` (teleport request / state),
`llviewermessage` (`TeleportLocal` / `TeleportFinish` / lure handling),
`llstartup` (progress screen).

Builds on: the existing teleport protocol (Phase 12).

Deps: [[viewer-ui-notification-host]].
