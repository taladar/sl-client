---
id: viewer-environment-pause-clouds
title: Pause cloud animation toggle
topic: viewer
status: ready
origin: main-menu survey (2026-07-23)
refs: [viewer-p22-4, viewer-phototools]
---

Context: [context/viewer.md](../context/viewer.md).

World ▸ Environment ▸ Pause Clouds: freeze the cloud-layer scroll so
clouds hold still — for photography, matching shots across takes, and
machinima. The rest of the sky (sun position, day cycle) keeps running.

Scope:

- A toggle that halts the cloud texture scroll/time advance in the cloud
  layer ([[viewer-p22-4]], done) without touching the day-cycle clock.
- Menu entry under World ▸ Environment; state survives environment
  preset switches, matching the reference.

Reference (Firestorm, read-only): `World.EnvSettings pause_clouds`
(`menu_viewer.xml` World ▸ Environment).

Builds on: the cloud layer renderer (done); a natural companion to the
[[viewer-phototools]] photography cluster.
