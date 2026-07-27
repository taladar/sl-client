---
id: viewer-world-test-harness
title: A headless fixture world — SlEvent in, SlCommand out
topic: viewer
status: ready
origin: user request (2026-07) — test in-world reactions without a server
points: 8
refs: [viewer-render-test-harness, viewer-ui-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

The in-world counterpart of `ui_test.rs`, built on the seam the
architecture already provides: everything downstream of the network
consumes public `SlEvent(SessionEvent)` messages (~55 readers) and
everything outbound is a `SlCommand(Command)` message; the socket lives
only in `sl-client-bevy`'s `drive` system. `sl-client-bevy/src/world.rs`'s
own tests already build exactly this app in miniature — this task scales
that pattern up.

Build `world_test.rs`: a `WorldTest` builder assembling TaskPool/Asset/
Input plugins, CPU-side asset registration (`render_test.rs`'s
`headless_app` proves prim meshing runs without a GPU), one
`Window`+`PrimaryWindow` with a settable cursor
(`Window::set_physical_cursor_position` — the gizmo/selection/menu systems
all require `windows.single()` + `cursor_position()`), a `ViewerCamera`
with hand-supplied camera values so `viewport_to_world`/
`world_to_viewport` work, `maintain_world` + the object/avatar/terrain
ingestion plugins, and a `SlCommand` recorder — **no** `SlClientPlugin`,
no sockets. Compose with the UI interaction stack from
[[viewer-ui-interaction-harness]] when a test needs both (build floater).

Fixture builders emit `SlEvent` sequences standing up one fat, unmissable
mesh per **target class** — prim, avatar, attachment, terrain, HUD, self —
each at a known position. Picking runs the real `ButtonInput` + cursor +
`MeshRayCast` path end-to-end: target *classification* (avatar vs object
vs attachment vs land) is reaction logic under test. Geometric
pick-*accuracy* (LOD, concave meshes, overlap) is explicitly out of scope;
each fixture is a target the cursor cannot miss.

The task's main structural work and one real unknown: carving a reusable
plugin subset out of `lib.rs::run()`'s inline ~70-plugin assembly — a
plugin that touches render resources at `build()` time needs splitting or
a headless flag.
