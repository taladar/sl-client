---
id: viewer-perf-terse-update-fast-path
title: Motion-only fast path for terse object updates
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

Terse motion updates are the **highest-frequency object event** in a
populated region — every walking avatar's attachments and every
physical/scripted mover generates `ImprovedTerseObjectUpdate`s at up to
sim frame rate. `sl-proto` folds each into a full `Object` snapshot and
emits `Event::ObjectUpdated(Box<Object>)`
(`sl-proto/src/session/methods.rs:2001-2024`), and the viewer routes it
through the same `apply_object` path as a full update
(`objects.rs:2029-2111`). For a *known* object that path, per terse
update:

- re-inserts `(Transform, SceneObject, ObjectDebugInfo)` on the object
  entity and the holder transform on the geometry entity — the transform
  is the one thing actually needed; `SceneObject` / recomputed
  `ObjectDebugInfo` re-inserts mark them `Changed` though they rarely
  differ;
- unconditionally re-runs `apply_render_materials` (allocates a
  `Vec<(u8, Uuid)>` when render materials exist),
  `apply_texture_animation`, `apply_light`, `apply_particles`,
  `apply_flexi`, `apply_reflection_probe`, `apply_physics` — and each
  helper issues `commands.entity(e).remove::<T>()` when its block is
  absent, so a plain moving prim generates **~5-6 no-op remove commands
  plus 2 multi-component inserts per motion packet**.

The geometry side is already right: a shape fingerprint
(`objects.rs:2054`) prevents re-tessellation on motion. The component
refresh is what lacks the equivalent gate.

## Proposed fix

Either (a) have `sl-proto` tag the event as motion-only (it knows it
came from a terse update) and take a fast path that writes only the
transforms; or (b) keep one event type but store per-sub-block
fingerprints (light/particles/flexi/probe/materials/texanim) on
`TrackedObject` — the same pattern as the existing shape fingerprint —
and skip each `apply_*` call, including its no-op remove, when its block
is unchanged. Also compare before re-inserting
`SceneObject`/`ObjectDebugInfo` so they stop being marked `Changed`
every packet. Option (b) is more robust (also dedupes full updates that
repeat identical blocks); (a) is simpler and matches the wire reality.

## Estimated impact

Medium, scaling linearly with mover count × update rate: on a busy
region (dozens of movers at 10-45 Hz each) this removes hundreds of
command-buffer entries, redundant `Changed` marks, and several `Vec`
allocations per frame from the main-thread command-apply point (command
application is serial, so this is directly frame-time relevant). On a
quiet region, negligible. Measure via [[viewer-profiling]] (command
apply span + `apply_object` zone counts) while several scripted movers
run on the test grid.

Confidence: high — event frequency verified in `sl-proto`, the
per-update helper cascade and no-op removes verified in `objects.rs`.
