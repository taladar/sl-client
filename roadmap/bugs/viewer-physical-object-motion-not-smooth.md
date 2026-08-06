---
id: viewer-physical-object-motion-not-smooth
title: Physical object (vehicle) motion is not as smooth as it should be between updates
topic: viewer
status: bugs
origin: user report during viewer-seated-avatar-vehicle-rubberband aditi testing (2026-08-06)
refs: [viewer-seated-avatar-vehicle-rubberband, viewer-p31-2, viewer-avatar-dead-reckoning-translation-rubberband]
---

Context: [context/viewer.md](../context/viewer.md).

A moving **vehicle** (a server-flagged physical root prim) does not render its
motion as smoothly as it should. Surfaced while confirming the seated-avatar fix
([[viewer-seated-avatar-vehicle-rubberband]]): once the seated rider was locked
rigidly to the seat, the rider is now as smooth as the **seat** — but the seat
(the vehicle) itself still moves with a visible lack of smoothness.

This is the **object** side of the same dead-reckoning story as the avatar
translation ease ([[viewer-avatar-dead-reckoning-translation-rubberband]]):
[`drive_physical_objects`](../../sl-client-bevy-viewer/src/physics.rs) (P31.2)
extrapolates a physical prim between `ObjectUpdate`s, then on each authoritative
update **hard-snaps** it back to truth (`reseed` + `place`). When prediction and
truth diverge, that per-update snap is a visible jump — the object analogue of
the avatar rubberband, except the object path was never given the P31.7-style
eased-translation treatment.

Likely contributors to investigate:

- **Snap-to-truth on each update** rather than easing the rendered position
  toward the authoritative / dead-reckoned position (the fix pattern is the
  translation counterpart of the avatar work — ease as a *delta*, snap only on
  region crossings / rebase).
- **Update cadence vs. viewer frame rate.** A vehicle's
  `ImprovedTerseObjectUpdate` stream may arrive sparsely / irregularly relative
  to the display rate; between updates the dead-reckon is only as good as the
  last-reported velocity + acceleration, and a scripted (non-physics) vehicle
  moved by `llSetPos` / keyframed motion carries **no** velocity, so it gets no
  interpolation at all and moves in discrete jumps. Check whether the test
  vehicle is physics-driven (`FLAGS_USE_PHYSICS`, dead-reckoned) or keyframed
  (only `update_objects` moves it — needs its own interpolation, cf. the
  reference's keyframe-motion path).
- **Angular smoothing.** Rotation is spun by angular velocity (`angular_step`)
  but, like the object translation, is snapped on each update — a turning
  vehicle may visibly hitch.

Not a regression from the seated-avatar fix — that change only reads the seat's
current-frame transform; it does not touch how the seat itself is driven.
