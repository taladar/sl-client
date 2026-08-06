---
id: viewer-seated-avatar-vehicle-rubberband
title: Seated avatar rubber-bands behind the vehicle it sits on instead of rigidly parenting
topic: viewer
status: done
origin: user report during viewer-avatar-tongue-protrudes aditi testing (2026-08-05)
refs: [viewer-avatar-dead-reckoning-translation-rubberband]
---

Context: [context/viewer.md](../context/viewer.md).

When an avatar sits on a moving **vehicle**, the avatar **lags behind and
catches up** (rubber-bands) rather than moving rigidly with the object as if
parented to it. A seated avatar's render position should be locked to the seat
(the object's frame), not dead-reckoned/eased toward a separately-tracked avatar
position.

Related to the general dead-reckoning ease
([[viewer-avatar-dead-reckoning-translation-rubberband]]) but specific to the
**seated-on-object** case: while seated, the avatar is effectively a child of
the vehicle, so its world transform should come from the object's transform +
the sit offset every frame, with no independent position smoothing. Check the
seated-placement path (`place_seated_avatars`) — it likely still eases the
anchor toward a network-updated avatar position instead of hard-following the
seat object, so vehicle motion (which updates the object faster/differently than
the avatar's own position stream) shows as lag-and-catch-up.

## Done

The filed premise (that `place_seated_avatars` still *eased* toward a
separately-tracked avatar position) was a guess: the seating-placement work
predates this bug and already **hard-follows** the seat. The actual cause was
the **one-frame `GlobalTransform` lag** (the same class of issue as the sibling
[[viewer-avatar-dead-reckoning-translation-rubberband]]):
`place_seated_avatars` ran in `Update` and read the seat's `GlobalTransform`,
which Bevy only recomputes in
`PostUpdate` — so it was **last frame's** seat pose. The vehicle mesh renders at
this frame's pose while the rider, read from the stale global, trailed by a
frame and lurched on each of the vehicle's dead-reckon / snap corrections — the
rubber-band (the "viewer FPS vs simulator FPS" the reporter suspected). The code
even documented that lag as assumed-invisible.

Fix (all viewer-side, no protocol change): `place_seated_avatars` now composes
the seat's **current-frame** world transform from the chain of local
`Transform`s up its `ChildOf` parents (new `seat_world_transform` — the manual
equivalent of propagation, but from this frame's locals), instead of reading the
frame-late `GlobalTransform`. The system is ordered after every mover that
writes a seat's local transform this frame (`update_objects`,
`drive_physical_objects`,
`drive_avatar_motion`). The anchor stays a top-level entity holding its world
pose, so name-tag / other readers are unchanged. The camera's own-seated branch
(`own_avatar_pose`) likewise now reads the anchor's current-frame local
`Transform` rather than its stale global. Unit test
`seat_world_transform_matches_propagation` checks the composition equals Bevy's
propagated `GlobalTransform` for both a root seat and a child-prim seat.

Live-verified on aditi (2026-08-06): sitting on a moving vehicle, the avatar no
longer rubber-bands — it now rides the seat rigidly. The vehicle *object itself*
still does not move as smoothly as desired; that is a separate follow-up,
[[viewer-physical-object-motion-not-smooth]] (the object-side dead-reckoning
ease, not touched by this change).
