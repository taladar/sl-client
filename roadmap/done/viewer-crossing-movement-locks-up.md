---
id: viewer-crossing-movement-locks-up
title: Movement locks up after a region crossing (stand-up anim, esp. onto lower terrain)
topic: viewer
status: done
origin: user report (2026-08-07), teleport/crossing live testing
refs: [viewer-seated-region-crossing, viewer-seamless-region-handover-objects,
  viewer-own-motion-timed-stops, viewer-own-avatar-vanishes-near-ground]
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

## Closed as not reproducible (2026-09-15)

The exact scenario no longer locks up at the current head, on the grid it was
reported on (the local OpenSim — the crossing work of 2026-08-07 was verified
there).

**The repro.** Default Region's terrain was raised 30 m (`terrain elevate 30`,
restored byte-identical from a `terrain save` afterwards), turning its borders
into 30 m cliffs, and the avatar walked off one into North Region. The OpenSim
journal shows the crossing mid-fall (`CrossAgentToNewRegionAsync: new
region=North Region … newpos=<226.6, 0.4, 69.9>`), and the viewer's
`SL_VIEWER_LOG_LOCOMOTION=1` log the reported animation sequence —
`falldown` → `standup` → `stand` — followed within a second by `walk`: the
avatar moved again. A fly-and-drop across a border (fall starting just before
the crossing) did not lock up either.

**Ruled out along the way:**

- *The simulator's landing hold.* A Second Life simulator waits for the
  viewer's `AGENT_CONTROL_FINISH_ANIM` before leaving a landing or pre-jump
  state, and sl-client never sent it. That gap was real and is now closed
  ([[viewer-own-motion-timed-stops]]), but it cannot have been this bug: OpenSim
  never reads the bit (its animator leaves the landing state on a 1 s timer,
  `ScenePresenceAnimator`, `landElapsed`), and aditi did not play `standup` at
  all, even after a 200 m fall.
- *The three suspects above.* No stale seat, no controls lost on the promoted
  circuit, no stuck ground floor — each would still lock the avatar in the
  repro, and none did.

Much of the crossing and handover path was reworked between the report and this
check (seated crossings, arrival facing, the teleport race fix), so it was most
likely fixed in passing; bisecting it would take a release build per step plus
a hand-driven repro each, which was not judged worth it.

The aditi fly-and-fall runs turned up an unrelated symptom, filed as
[[viewer-own-avatar-vanishes-near-ground]].
