---
id: viewer-audit-plugins-own-their-schedule
title: Most viewer crates export loose systems instead of owning a plugin
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 13
refs: [viewer-audit-plugin-resource-registration, viewer-audit-system-ordering-claims]
---

Context: [context/viewer.md](../context/viewer.md).

Only `PhysicsPlugin`, `MediaPrimPlugin`, `CameraPlugin`, `InputActionPlugin`,
`InputContextPlugin`, `ParcelBordersPlugin`, `SlTonemapPlugin` and
`TransparencyOrderPlugin` exist. The rest of the world and feature crates export
loose `pub fn` systems, wired by a single ~900-line `add_systems` in
`sl-client-bevy-viewer/src/lib.rs:1927+` — whose comments repeatedly say tuples
are nested "to stay within Bevy's per-tuple system limit".

Two consequences:

- cross-crate invariants live in the **binary** rather than in the crate that
  owns them, which is directly why [[viewer-audit-plugin-resource-registration]]
  and [[viewer-audit-system-ordering-claims]] exist;
- no crate can be dropped into a test `App` on its own, which is a large part of
  why the viewer crates' test ratios are what they are.

`PhysicsPlugin` (`sl-viewer-world-view/src/physics.rs:124`) is the model: 14
systems with explicit, commented `.after()` / `.before()` edges.

Scope: give each crate a plugin that registers its own resources, systems and
ordering edges. The binary then composes plugins rather than systems.
