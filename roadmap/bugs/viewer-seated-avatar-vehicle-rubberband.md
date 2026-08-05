---
id: viewer-seated-avatar-vehicle-rubberband
title: Seated avatar rubber-bands behind the vehicle it sits on instead of rigidly parenting
topic: viewer
status: bugs
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
