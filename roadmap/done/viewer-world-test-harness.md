---
id: viewer-world-test-harness
title: A headless fixture world — SlEvent in, SlCommand out
topic: viewer
status: done
origin: user request (2026-07) — test in-world reactions without a server
points: 8
refs: [viewer-render-test-harness, viewer-ui-test-harness, viewer-cpu-pick-resolver]
blocked_by: [viewer-plugin-groups]
---

Context: [context/viewer.md](../context/viewer.md),
[context/testing.md](../context/testing.md).

Done (2026-08-31): `world_test.rs` landed — `world_app()` (the
testkit input stack + visibility propagation + the world fold with the
CPU resolver + every resource / message the group's systems validate
against), `world_app_with_edit()` adding `EditGizmoPlugin`, the
hand-filled `ViewerCamera` installer, a movable-prim fixture over
`objects::fixture_object`, `world_to_viewport`, and the first two
consumer tests (pie target, gizmo drag). The missing-resource inventory
in `world_app` documents which resources / messages each absent plugin
group owns — grown empirically via `RUST_BACKTRACE=full`, whose
monomorphised `run_unsafe` frame names a failing system's full parameter
list without needing the `bevy/debug` feature. The fixture classes
landed 2026-08-31: `seed_avatar` (placeholder sphere; own avatar via
`SlIdentity.agent_id`), `seed_attachment` (nibble-swapped point in the
state byte), `seed_terrain` (one flat land patch), plus the aim helpers
(`scene_position_of`, `avatar_position_of`, `terrain_centre`).
The HUD fixture followed the same day, unblocked by the vendored
character assets: `world_app_with_hud()` loads the real
`AvatarAssetLibrary` from `viewer-assets/character/`, adds the
render-layer propagation the HUD pick paths filter by, and
`install_hud_camera_projection` hand-fills the orthographic HUD
camera's computed values.

The `with_ui` composition and the helper surface followed the same day:
`world_app_with_ui()` stands the layout stack and the UI half of the
interaction stack over `world_app_with_hud()` — over the HUD one
because that camera carries `IsDefaultUiCamera`, which is what decides
where the UI root reads its size from, and a UI composed onto a world
with no such marker would target whichever camera won a `max_by_key`.
`entity_of`, `drain_commands` and `select_by_click` landed with it,
`select_by_click` driving the real `EditSelectionPlugin` gesture (whose
one absent resource is the combo widget's `UiPointerClaim`). The
consumer test is the whole loop in one: an `ObjectAdded` streams in, a
right-click opens the real object pie, a click on the `Touch` label the
user sees sends one `TouchObject` on the wire.

One correction the selection click forced, worth carrying: the fixture
world had no camera **frustum**, because `bevy_camera`'s `CameraPlugin`
owns `update_frusta` and lives in the group the world tier leaves out.
Without one `check_visibility` culls everything, `ViewVisibility` never
becomes true, and every ray cast left at the default
`RayCastVisibility::VisibleInView` — `ObjectPicker::pick`, so the whole
left-click path — silently hits nothing, while the pick resolver's own
`Visible` cast goes on working. `world_app` runs `update_frusta` now.

Blocked on [[viewer-plugin-groups]] (2026-08-30): the "one real unknown"
below — carving a reusable plugin subset out of `run()` — is that task, so
this one starts from its `ViewerWorldPlugins`. World picks are a GPU
render, so target classification also needs [[viewer-cpu-pick-resolver]].
The `WorldTest` builder, `Fixture` enum and helpers are specified in
`context/testing.md`'s plan; the fake-grid tier is *not* a smoke tier any
more — a test lives in the lowest tier that can produce its failure.

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
