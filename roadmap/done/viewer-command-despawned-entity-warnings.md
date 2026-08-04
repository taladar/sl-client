---
id: viewer-command-despawned-entity-warnings
title: "\"Entity despawned\" command warnings during a plain scene run"
topic: viewer
status: done
origin: observed during the viewer-sun-disc-grey aditi verification (2026-08-04)
refs: [viewer-floater-update-in-place]
---

Context: [context/viewer.md](../context/viewer.md).

During ordinary aditi runs (login + scene streaming, **no profile / property
floater open**) the log emits a steady trickle of:

```text
WARN bevy_ecs::error::handler: Encountered an error in command
  `<Enable the debug feature to see the name>`: Entity despawned: The entity
  with ID 33261v2 is invalid; its index now has generation 3.
Note that interacting with a despawned entity is the most common cause of this
error but there are others
```

A queued command targets an entity that was despawned before the command
flushed — a same-/cross-frame despawn race. It currently surfaces only as a
`bevy_ecs::error::handler` **WARN** (Bevy's default error handler), i.e. it is
being *tolerated*, not fixed — a downgraded error must not be left masking a
root cause; fix it at the source.

This is **distinct from** [[viewer-floater-update-in-place]]: that task is the
profile / property *floater* churn (defused with `try_insert`), but these
warnings fire with no such floater open, so the racing system is elsewhere —
most likely the **object / avatar / attachment lifecycle** (an entity despawned
on `ObjectRemoved` / LOD swap / attachment drop while a queued command — a
material set, a child insert, a transform write — still references it).

## Resolved

Pinned with a temporary `bevy/debug` feature build (the feature only wires up
`DebugName` strings — it is behaviour-neutral, so it did not change the race):
the handler named the command as an `insert` of
`(Transform, SceneObject, ObjectDebugInfo, ObjectSlMotion)`, which is
`apply_object`'s **known-object update** branch (`objects.rs`,
`commands.entity(existing.entity).insert((…))`). The offending entity reported
the `Invalid` (generation-mismatch) error variant — `ID 4200v0 … now has
generation 1` — **not** `ValidButNotSpawned`, so it was *not* a same-frame
despawn-before-flush race: it was a **stored entity that outlived a despawn and
a slot reuse** while still in `ObjectState.objects`, and it fired right at
initial scene streaming (so the original "same-/cross-frame despawn race" and
"steady trickle needing a populated region" framing above were both off — it is
a stale-tracked-entity bug that reproduces on a plain solo login).

Root cause: an objects.rs entity dies one of two ways — our own `remove_object`
(which drops it from the map) or **Bevy's recursive despawn taking it with its
parent**. The second path is untracked: a linkset child, or a worn attachment
hanging off an avatar's skeleton-joint node, is despawned the instant its parent
(a removed linkset root / a departed avatar) despawns, with no `remove_object`
for it — so its entry lingers in `ObjectState.objects`, and the next
`ObjectUpdated` re-inserts on the dead (later slot-reused) entity.

Fix (`sl-client-bevy-viewer/src/objects.rs`):

- `drop_stale_tracked_entity` — before the update branch, `apply_object` checks
  the tracked entity with `Commands::get_entity(..).is_ok()`; a **dead**
  entity's entry is dropped and the object falls through to the spawn path,
  respawning it (a live entity is never touched, so no on-screen object's
  transform / material write is dropped — not the cosmetic-masking
  anti-pattern). Since a dead tracked entity means its parent hierarchy is gone,
  respawning is correct: the simulator either keeps streaming it (restored) or
  its imminent `KillObject` reaps it.
- `remove_object` now uses `try_despawn` for the symmetric case (a child /
  attachment whose `KillObject` arrives after its parent's hierarchy despawn
  already took it).

Verification: the intermittent live race (it fired in only ~1 of 3 pre-fix
~2-minute aditi logins) is pinned deterministically by a client-side test —
`apply_object_respawns_a_child_despawned_out_from_under_the_map` drives the real
`apply_object` path and, with the guard removed, **panics in
`bevy_ecs::error::handler`** (the exact "Entity despawned" site); with the guard
it respawns a fresh, live, correctly re-parented child. Two focused unit tests
(`stale_guard_drops_a_hierarchy_despawned_tracked_object`,
`stale_guard_keeps_a_live_tracked_object`) lock the guard's decision against a
real Bevy hierarchy despawn. Post-fix aditi logins showed no command warnings
and no rendering regression.
