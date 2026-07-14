---
id: viewer-teleport-flow
title: Teleport flow — offers, acceptance & progress
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-notifications-dialogs, viewer-world-map, viewer-ui-framework]
---

Context: [context/viewer.md](../context/viewer.md).

The user-facing teleport experience: **initiate** a teleport (from the map, a
landmark, a SLURL, or "teleport home"), the teleport **progress** screen with
its state messages and cancel, and **receiving / accepting / declining**
teleport offers and lures from other residents (offer a teleport, send a lure).

The teleport **protocol** is already done and tested (see the Phase-12
teleport conformance cases — local, cross-region, failed, and offer/accept);
this stub is the viewer flow + UI on top. The incoming-offer dialog itself is a
case of the notifications system.

Reference (Firestorm, read-only): `llagent` (teleport request / state),
`llviewermessage` (`TeleportLocal` / `TeleportFinish` / lure handling),
`llfloatermap` / `llfloaterworldmap` (map teleport), `llstartup` (progress).

Builds on: the existing teleport protocol (Phase 12).

Deps: [[viewer-notifications-dialogs]] (offer dialog), [[viewer-world-map]] (map
teleport), [[viewer-ui-framework]].
