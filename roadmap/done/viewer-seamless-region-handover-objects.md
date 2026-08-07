---
id: viewer-seamless-region-handover-objects
title: Seamless region handover for world objects (neighbour render + rebase, not purge)
topic: viewer
status: done
origin: surfaced during teleport live testing (2026-08-07) once cross-region teleport first worked
refs: [viewer-seated-region-crossing, viewer-avatar-dead-reckoning-translation-rubberband]
---

Context: [context/viewer.md](../context/viewer.md).

Once cross-region **teleport** first worked (the `begin_handover` promote-child
fix), it exposed that the viewer never handles **world objects** across a
root-region change — for a **crossing** *or* a **teleport**. Symptoms:

- Teleporting to a neighbour: the region you left's objects stay rendered at
  their **old Bevy offset**, so they appear piled into the new region.
- Objects do **not** rebase to the new relative offset in **either** case
  (crossing or teleport) — only terrain + the camera do
  (`terrain.rs::recenter_terrain` shifts the flycam by `-shift` and re-places
  patches; objects get nothing).

The user's intent (a crossing *is* a seamless teleport, just not surfaced):
**keep and rebase, do not purge**. Purging a neighbour's objects on a promote is
pointless — the region we left becomes a child circuit that would immediately
re-stream them.

## Architecture found

- `objects.rs::object_transform` places a **root** object at the raw
  `sl_to_bevy_vec(&object.motion.position)` — **no per-region offset**. (Only
  coarse *avatar* dots offset by region metres relative to the origin,
  `avatars.rs` ~2407.) So neighbour-region objects have no origin-relative
  placement, and nothing rebases them when the origin moves.
- `recenter_terrain` is the model to mirror: on a root change it shifts the
  flycam by `-shift` and re-places terrain; objects need the analogous shift.
- Session: `promote_child_to_root` (crossing) keeps
  `objects`/`terrain`/`regions` and demotes the old root to a child (so it keeps
  streaming); `begin_handover` (teleport) currently **clears** them. For a
  *neighbour* (promote) teleport it should behave like the crossing (keep +
  rebase, old root → child); only a **distant** (fresh-circuit) teleport should
  clear.

## Scope

1. **Neighbour object streaming/rendering.** Confirm whether child-circuit
   object updates are surfaced/rendered at all; if not, render them, offset by
   the region's global metres relative to the current origin (like the coarse
   dot offset), so the adjacent region is visible as you approach it.
2. **Object rebase on origin change.** When the root region changes, shift every
   world-root object entity by the same `-shift` `recenter_terrain` applies to
   the camera (a uniform delta — the origin moved once for everything), so a
   crossing and a neighbour teleport both keep objects correctly placed.
3. **Session keep-vs-clear.** `begin_handover`'s promote branch should keep
   `objects`/`terrain`/`regions` and demote the old root to a child (reuse
   `promote_child_to_root`'s world handling), adding only the teleport's
   unseat + `drop_inworld_grants`. The fresh branch keeps today's
   clear-everything.
4. **Distant-teleport despawn.** When the session *does* clear (fresh circuit),
   the viewer must despawn its object mirror (it currently doesn't — the session
   clearing its cache emits no removals), or the stale entities linger.

Reference (Firestorm, read-only): `LLWorld::updateRegions` /
`LLViewerRegion` origin handling, the agent-region-crossing object handoff.
