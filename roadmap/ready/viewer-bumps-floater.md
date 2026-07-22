---
id: viewer-bumps-floater
title: Bumps, pushes & hits floater
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [api-g13]
---

Context: [context/viewer.md](../context/viewer.md).

The "Bumps, Pushes & Hits" log: a floater listing who collided with, pushed
or hit the agent, with type and timestamp — the harassment-evidence tool. The
receive side is already done: [[api-g13]] decodes `MeanCollisionAlert` into
typed events (perpetrator, type bump/push/selected/scripted/physical, time,
magnitude); this task is the floater over a retained ring buffer of those
events, name-resolved rows, and a context-menu jump to the profile / Report
Abuse ([[viewer-report-abuse]]).

Reference (Firestorm, read-only): `llfloaterbump`, `floater_bumps.xml`.

Builds on: [[api-g13]] `MeanCollisionAlert` events.
