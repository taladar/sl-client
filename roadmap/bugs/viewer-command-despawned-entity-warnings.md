---
id: viewer-command-despawned-entity-warnings
title: "\"Entity despawned\" command warnings during a plain scene run"
topic: viewer
status: bugs
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

To investigate:

- The command name is hidden (`<Enable the debug feature to see the name>`).
  Build with Bevy's `track_location` / debug feature (or temporarily name the
  offending systems) so the handler prints **which** command is touching the
  dead entity — that pins the system.
- Once identified, fix at the source: either order the command before the
  despawn, make it `try_insert` / existence-checked (the tolerant-command
  pattern), or stop despawning the entity out from under an in-flight update.
- Confirm it is not merely cosmetic: a dropped material/transform write on a
  live entity would be a real visual bug, not just log noise.

Reproducible on any login (localhost or aditi) with scene streaming, so a
headless run that greps the log for the warning can gate a fix.
