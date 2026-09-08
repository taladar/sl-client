---
id: viewer-teleport-never-resets-the-world
title: A teleport never sets world_reset, so the world-scoped purge may be dead code
topic: viewer
origin: measured while fixing the full-stack teleport flake (2026-09-08)
status: bugs
refs: [viewer-full-stack-teleport-leftover-race]
---

Context: [context/viewer.md](../context/viewer.md).

`Session::begin_handover` decides whether a teleport clears the scene mirror:

```text
let dest_is_child = self.children.contains_key(&dest);
let dest_is_adjacent = self
    .region_handle()
    .is_some_and(|source| source.is_adjacent_to(region_handle));
let world_reset = !dest_is_child && !dest_is_adjacent;
```

The comment beside it says the reset is for "a distant teleport to an
unconnected region". **The destination is never unconnected by then.** A
teleport announces the destination to the client *before* it finishes: the
event-queue trio is `EnableSimulator` → `EstablishAgentCommunication` →
`TeleportFinish`, and the client's `EnableSimulator` arm calls
`open_child_circuit`. By the time `TeleportFinish` reaches `begin_handover` the
destination is always in `children`, so `dest_is_child` is true and
`world_reset` is false.

Measured on the fake grid: a teleport ten grid cells away — as distant and as
unconnected as the harness can make it — reports

```text
world_reset flags seen: [false]
```

## Why this is not just a fake-grid quirk

OpenSim's `EntityTransferModule` V2 transfer announces the destination the same
way, for the same reason (the client has to open the child circuit and POST the
seed before it can be promoted). So the ordering that makes `dest_is_child` true
is the protocol's, not the fake grid's.

If that holds on a live grid too, then **every** `WorldScoped` store's
`purge_world` is unreachable on the teleport path: `ObjectState`, `AvatarState`,
`TerrainState`, `MediaData`, `ObjectCostModel`, `ObjectPhysicsShapes`,
`RegionTimeDilation`, `PendingObjectEvents`, `RiggedBindSkipLog`. What actually
clears a departed region today is the **retirement of its circuit**, which is a
different mechanism with different timing (it waits for the destination to
confirm the arrival) and different coverage.

## What to check first

1. **Confirm on aditi.** Log the `world_reset` flag across a long-distance
   teleport on a live grid. That is the whole question; everything below only
   matters if it is false there too.
2. **Whether the reset is still wanted.** If circuit retirement already clears
   everything the purge would, the flag and the machinery behind it are dead
   weight and should be removed rather than fixed — that is a real possibility
   and would be the better outcome.
3. **Whether anything is actually leaking.** The purge exists because something
   was believed to survive a distant teleport. If the stores are keyed by
   circuit (as `ObjectState` is) retirement covers them; a store keyed by
   *agent* or by nothing (`RiggedBindSkipLog`, `ObjectCostModel`) may genuinely
   be carrying stale entries across a teleport right now. That is the concrete
   thing to look for, and the reason this is filed as a bug rather than a
   cleanup.

## Why it went unnoticed

Nothing asserts the flag. `viewer-full-stack-teleport-leftover-race` looked like
it did — it asserted that the departed region's objects were gone and blamed
"the scene was emptied around them rather than purged" — but the objects were
leaving by circuit retirement all along, so the assertion passed while the purge
it named never ran.
