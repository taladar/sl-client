---
id: viewer-teleport-never-resets-the-world
title: A teleport never set world_reset, because the fake grid announced the destination first
topic: viewer
origin: measured while fixing the full-stack teleport flake (2026-09-08)
status: done
refs:
  - viewer-full-stack-teleport-leftover-race
  - viewer-teleport-reset-defeated-by-a-v1-grid
  - viewer-audit-system-ordering-claims
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

Measured on the fake grid, a teleport ten grid cells away — as distant and as
unconnected as the harness could make it — reported

```text
world_reset flags seen: [false]
```

so the purge behind the flag looked like dead code, and every `WorldScoped`
store's `purge_world` looked unreachable.

## What it actually was

**The fake grid, not the client.** `sl-fake-grid`'s teleport put the event-queue
pair `EnableSimulator` + `EstablishAgentCommunication` in front of the
`TeleportFinish`, so the client had already opened the destination as a child
circuit by the time the finish arrived: `dest_is_child` was true, and the world
was kept. Its own module docs claimed to mirror OpenSim's `TransferAgent_V2`,
which is precisely the transfer that does **not** do this:

```csharp
// New protocol: send TP Finish directly, without prior ES or EAC. That's what
// happens in the Linden grid
if (m_eqModule != null)
    m_eqModule.TeleportFinishEvent(destinationHandle, 13, endPoint, 0, ...);
```

— `EntityTransferModule.TransferAgent_V2`. Only the legacy `TransferAgent_V1`
prefixes the finish with that pair, and even there only for a destination
`OutSideViewRange`; V2 is taken whenever the destination simulator speaks
protocol 0.2 or newer, which every current OpenSim does.

The reference viewer agrees: `process_teleport_finish` sends `UseCircuitCode`
to the address the finish names *unconditionally*, without first asking whether
it already holds that region. A viewer that had been handed the destination
ahead of time would not need to.

So the flag was right and the measuring instrument was wrong. The fix is in
`sl-fake-grid::teleport`: no announcement before the finish, exactly as V2 and
the Linden grid do it. A destination that is genuinely a **neighbour** was of
course announced long before, by `sl-fake-grid::neighbours`, and that path still
reuses its session.

## What the flag was hiding

Turning it on for the first time broke three full-stack tests: a teleport landed
in an empty region. The purge was deleting the destination's arrival burst.

`detect_world_reset` and the object / terrain / avatar folds all read the same
`SlEvent` channel, and **none of them was ordered against the system that writes
it** (`sl_client_bevy`'s `drive`). The scheduler was free to run the detector
before the writer and the object fold after it, which puts the two a whole frame
apart on the same batch:

```text
frame N    drive writes [RegionChanged{reset}, ObjectAdded x26]
           update_objects folds all 26 into ObjectState
frame N+1  detect_world_reset finally sees RegionChanged
           purge_world empties ObjectState — the destination's scene with it
```

Measured exactly that way: 26 `ObjectAdded` events recorded for the destination,
`ObjectState` empty at capture, origin and identity both correctly on the
destination.

The fix is an ordering edge and a name to hang it on:
`SlClientSystems::SessionDrained` marks the point at which the session's events
for this frame have been written and the region mirror folded, and
`WorldResetSystems::Detect` is pinned after it. Every reader ordered after that
set agrees on which frame an event belongs to; the purge then runs before the
folds it must precede, which the existing `Purge.before(recenter_*)` edges
already arranged. Compare [[viewer-audit-system-ordering-claims]], which is the
same class of defect found statically.

This is the part worth remembering: the flag had never been true against a
grid, so nothing downstream of it had ever run. A purge that cannot fire is not
merely inert — it is untested, and it was wrong.

## The three questions this task asked

1. **Confirm on a live grid.** Not run as a live experiment; answered from the
   two references above, which agree and are the grids in question. The
   remaining live check is now cheap to fold into any aditi session, because
   `detect_world_reset` logs which way the flag fell at `info` on every region
   change: a distant teleport must say *"the world is purged and rebuilt"*, a
   step next door *"the world is kept and re-based"*.
2. **Is the reset still wanted?** Yes, and it is not redundant with circuit
   retirement. Retirement clears what is keyed by circuit; the purge covers the
   stores keyed by agent or by nothing, and it runs at the arrival rather than
   whenever the departed sim gets around to its `DisableSimulator`. Note the
   reference viewer has no equivalent — `LLWorld` drops a region *only* on
   `DisableSimulator` — so this is a deliberate divergence, and one that only
   pays for itself if the flag actually fires. It now does.
3. **Was anything leaking?** Everything world-scoped was, on every distant
   teleport, for as long as the tests only ever saw this grid — all eleven
   registered stores: `ObjectState`, `AvatarState`, `TerrainState`,
   `TerrainTextures`, `WaterState`, `MediaData`, `ObjectCostModel`,
   `ObjectPhysicsShapes`, `RegionTimeDilation`, `PendingObjectEvents`,
   `RiggedBindSkipLog`. The ones keyed by circuit were swept up by retirement a
   moment later; `RiggedBindSkipLog` and `ObjectCostModel` are not keyed by
   circuit and were not.

## What now pins it

- `sl-fake-grid`'s `inter_region_teleport_over_loopback` asserts
  `world_reset == true` for the ten-regions-east hop, and panics outright if a
  `NeighborSeed` for the destination arrives before the finish.
- `a_teleport_to_a_neighbour_reuses_its_child_session` asserts the other half,
  `world_reset == false` next door — without both, "always reset" and "never
  reset" each pass a test.
- The full-stack teleport tests (`a_teleport_leaks_nothing_between_regions`,
  `a_teleport_keeps_the_subject_where_it_is`) run against a destination ten
  regions east, so from here they exercise the purge rather than only circuit
  retirement.

## What is left

[[viewer-teleport-reset-defeated-by-a-v1-grid]] — the client's `dest_is_child`
test still cannot tell a V1 grid's pre-announced destination from a neighbour
it has been holding, so on such a grid the reset is still suppressed.
