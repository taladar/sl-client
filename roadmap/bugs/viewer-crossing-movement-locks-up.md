---
id: viewer-crossing-movement-locks-up
title: Movement locks up after a region crossing (stand-up anim, esp. onto lower terrain)
topic: viewer
status: bugs
origin: user report (2026-08-07), teleport/crossing live testing
refs: [viewer-seated-region-crossing, viewer-seamless-region-handover-objects]
---

Context: [context/viewer.md](../context/viewer.md).

After walking/flying across a region border — reproduced crossing onto a
**cliff** where the destination ground is **lower** — the avatar arrived
(fell down the cliff correctly), then **played a stand-up animation** and
afterwards **could not move** (input no longer drove the avatar).

Not from the teleport-UI / ease work (that touches only the teleport progress
overlay, the teleport protocol handover, and the *rendered* position — none of
which drive the walk animation, controls, or the sit state). Suspects to
investigate:

- **A transient sit/stand on the crossing.** A "stand up" animation implies the
  agent briefly read as seated across the handover (cf. the transient
  unsit/resit in [[viewer-seated-region-crossing]]), even though the user was
  walking. If `SitState`/the ground-sit flag is left set after the crossing, the
  movement path may route controls to a (non-existent) seat instead of the
  avatar.
- **Controls not re-driven on the promoted circuit.** After
  `promote_child_to_root` the root circuit is the promoted child; confirm the
  movement system keeps sending `AgentUpdate` control flags on the new root so
  keyboard movement still reaches the sim.
- **Terrain-height interaction at the border.** The lower destination terrain +
  the avatar ground-floor (`physics.rs::avatar_ground_floor`,
  `terrain.land_height`) may clamp/stick the avatar if the destination patch is
  not yet loaded when it arrives.

Repro: cross a border where the destination region's ground is markedly lower (a
cliff), watch for the stand-up animation and the subsequent inability to move.
