---
id: protocol-teleport-deferred-teardown-handover
title: Defer handover teardown until the destination confirms (+ overlap safety)
topic: protocol
status: done
origin: user report + investigation (2026-08-07); user chose the reference-
  faithful "defer teardown" approach over a force-recover timeout
refs: [protocol-teleport-timeout-strands-child-circuits, test-handover-mock-grid-harness, viewer-seamless-region-handover-objects]
---

Context: [context/viewer.md](../context/viewer.md).

Fixes [[protocol-teleport-timeout-strands-child-circuits]]. Today
`begin_handover` tears down the source region (retarget/clear or promote+demote)
the moment
`TeleportFinish` arrives, before the destination confirms — so a lost
handshake strands the session with its child circuits gone. Instead, **keep the
source region and its children live until the destination's
`AgentMovementComplete` confirms**, then commit; on timeout/cancel, drop only
the pending destination and stay put in the fully-intact source (a clean
`TeleportFailed`, retry works normally).

## Design

- During a teleport the destination lives as a **child** circuit until it
  confirms (an address is strictly root-or-child today, and `complete_arrival`
  hard-assumes the arrival is already root — so a new `pending_handover:
  Option<PendingHandover>` field + a commit path on the child are needed).
- `begin_handover` (on `TeleportFinish`): ensure a child circuit to `dest`
  (promote case: already one; fresh case: `open_child_circuit`), send
  `CompleteAgentMovement` on it, record `pending_handover { dest, region_handle,
  seed, from_child }`, and **change nothing else**. `state` stays `Teleporting`
  so the existing 30 s teleport timer still guards it.
- **Commit** when the destination's `AgentMovementComplete` (or a re-sent
  `RegionHandshake`) arrives on that child: swap it to root and tear down the
  source — demote old root + keep neighbours (`from_child`,
  `world_reset=false`) or drop everything (fresh, `world_reset=true`) — do the
  unseat + `drop_inworld_grants`, set the seed, emit `RegionChanged`. This
  subsumes the current `begin_handover` promote-vs-fresh teardown (it just moves
  to commit time).
- **Timeout / cancel** (`run_timeout` teleport timer; `cancel_teleport`;
  `teleport_to` supersede): drop the pending dest circuit (only if we opened
  it), clear `pending_handover`, stay `Active` in the untouched source, emit
  `TeleportFailed`. Make `cancel_teleport` (and the watchdog path through it)
  actually recover from the pending state.

## Overlap safety (single in-flight handover invariant)

Any new transfer must cleanly abort/finalize the previous before starting:

- Teleport while a teleport is pending -> supersede (abort pending, start new).
- Teleport while a crossing is finalizing (`AwaitingHandshake`) -> finalize the
  crossing first, then teleport.
- **Crossing while a crossing is finalizing** -> finalize the first, then
  promote to the new dest. This is the classic **double crossing on a vehicle
  near a region corner** on the SL grid (two `CrossedRegion`s in quick
  succession) — the primary overlap case to get right.
- Crossing while a teleport is pending -> the sim already moved us, so the
  crossing wins: abort the pending teleport, promote to the crossing dest.
- Duplicate `TeleportFinish` (same dest) -> no-op; different dest -> re-target.

## Testing

Drive [[test-handover-mock-grid-harness]] for every combination: fresh & promote
teleport, crossing, timeout at each phase, lost `AgentMovementComplete` / lost
`RegionHandshake`, cancel mid-flight, superseding teleport, the corner
double-crossing, and the regression that a timed-out teleport leaves the source
region + child circuits intact so the retry promotes normally. Update the ~15
existing teleport/crossing lifecycle tests to the deferred-commit flow.
