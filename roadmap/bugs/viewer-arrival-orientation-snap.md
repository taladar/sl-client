---
id: viewer-arrival-orientation-snap
title: Avatar arrives facing the wrong way then snaps to the correct orientation (rotates the whole minimap)
topic: viewer
status: bugs
origin: user report while live-testing the event-queue redesign on local OpenSim (2026-08-07)
---

Context: [context/viewer.md](../context/viewer.md).

On arriving in a region (crossing or teleport) the avatar appears in some
default / stale orientation and then, a moment later, **turns to its correct
facing**. Because the minimap is oriented to the avatar's heading, the whole
minimap rotates on every arrival — a jarring visual.

The arrival orientation should be applied **directly** on arrival rather than
being corrected a frame or more later. Likely the initial placement uses a
default/last rotation and only the subsequent `AgentMovementComplete` `look_at`
(or the first full `ObjectUpdate` for the own avatar) sets the true facing.

Investigate:

- The arrival `look_at` from `AgentMovementComplete` / `CrossedRegion` /
  `TeleportFinish` — is it applied to the avatar + camera + minimap heading at
  the moment of the `RegionChanged`, or only once the own avatar's full object
  re-streams from the new root?
- The own-avatar placement path (`avatars.rs` `apply_object` /
  `body_root_transform`) and the minimap heading source — make the heading track
  the applied arrival orientation immediately.

Now easy to observe since crossings/teleports complete cleanly (the event-queue
single-worker redesign removed the freezes that previously masked this).
