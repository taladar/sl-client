---
id: viewer-perf-terse-update-fast-path
title: Motion-only fast path for terse object updates
topic: viewer
status: done
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling, viewer-perf-object-update-coalesce]
---

Context: [context/viewer.md](../context/viewer.md).

> 2026-08-10: the **backlog** half of this is now covered —
> [[viewer-perf-object-update-coalesce]] merges repeated updates for one
> still-queued object into a single newest-snapshot build under the spawn
> budget. This task remains for the **inline** (no-backlog) path below,
> which still runs the full helper cascade per terse packet; its
> "per-object rebuild rate cap" follow-up idea is the same territory.

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

## Done (2026-08-10, `performance` branch) — option (b)

Implemented as the per-sub-block comparison (option b), which also dedupes
full updates that repeat identical blocks:

- `TrackedObject::non_motion_blocks_changed` compares exactly what the
  helper cascade reads against the last applied update: the extra params
  (light / particles / flexi / probe / render materials — already stored),
  the newly stored `texture_animation` / `text` / `text_color`, the update
  flags (physics toggle), the material byte, and the linkset / attachment
  identity. The merged-snapshot semantics make the compare exact.
- When nothing changed (the terse motion case), the whole helper cascade —
  including every absent block's no-op remove and the four block
  derivations — is skipped; a **physical** mover still re-seeds its
  dead-reckoning via the new `refresh_physical_motion` (insert-only, no
  remove side).
- `SceneObject` re-inserts only on a block change; `ObjectDebugInfo` /
  `ObjectSlMotion` write through `set_if_neq` (a repeated identical full
  update no longer marks them changed); the world-root marker sync moved
  behind the same gate (an `is_root` change always trips it).
- `reconcile_parent` no longer re-inserts `ChildOf` on an already-parented
  child unless the parent actually changed — previously one hierarchy
  change-mark per motion packet for every moving linkset child.

Unit-tested (`non_motion_gate_ignores_motion_and_tracks_block_inputs`:
motion-only passes the gate; text / flags / material / linkset changes trip
it) plus the existing objects suite. Live verification first surfaced (and
was briefly blocked by) a pre-existing debug-build crash, root-caused and
fixed upstream as
[[viewer-flair-style-panic-on-caps-failure-notification]]; with that patch
in place, debug-build logins on the local grid run the full login → render
(terrain / prims / avatar / lights / name tag intact, terse updates
streaming through the fast path) → screenshot → clean-logout cycle. The
Tracy command-apply / `apply_object` per-event measure with scripted movers
remains the standard follow-up profiling exercise.
