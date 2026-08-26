---
id: viewer-physical-object-motion-not-smooth
title: Physical object (vehicle) motion is not as smooth as it should be between updates
topic: viewer
status: done
origin: user report during viewer-seated-avatar-vehicle-rubberband aditi testing (2026-08-06)
refs: [viewer-seated-avatar-vehicle-rubberband, viewer-p31-2, viewer-avatar-dead-reckoning-translation-rubberband, viewer-sit-camera-vehicle-frame-lag, viewer-scripted-followcam-llsetcameraparams, viewer-object-ping-forward-interpolation, viewer-agentupdate-cadence-effects]
---

Context: [context/viewer.md](../context/viewer.md).

A moving **vehicle** (a server-flagged physical root prim) does not render its
motion as smoothly as it should. Surfaced while confirming the seated-avatar fix
([[viewer-seated-avatar-vehicle-rubberband]]): once the seated rider was locked
rigidly to the seat, the rider is now as smooth as the **seat** — but the seat
(the vehicle) itself still moves with a visible lack of smoothness.

This is the **object** side of the same dead-reckoning story as the avatar
translation ease ([[viewer-avatar-dead-reckoning-translation-rubberband]]):
[`drive_physical_objects`](../../sl-viewer-world-view/src/physics.rs) (P31.2)
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

## Done

Three of the filed contributors were addressed, and live aditi testing (driving
the "Kart 1.0") revealed that the **dominant** perceived-smoothness problem was
not the object dead-reckoning at all but the **camera** — split out and fixed
separately.

**Snap-to-truth on each update (object dead-reckoning ease).** Implemented as
the object counterpart of the avatar's P31.7 rotation ease, but as a decaying
*residual* rather than an absolute ease (so a steadily dead-reckoned object
carries no velocity-proportional standing lag): `drive_physical_objects` no
longer hard-snaps (`place`) on each `ObjectUpdate`. On reseed it re-aims a
residual (`reaim_residual`) so the rendered pose stays continuous, then
`place_smoothed` eases the correction away over `OBJECT_SMOOTHING_TAU_SECS`
(~100 ms) — covering **both** translation (the object analogue the avatar path
still lacks) and rotation (so a turning object no longer hitches). A
region-scale gap (> `OBJECT_SNAP_DISTANCE_M`, 32 m — a region crossing / rebase
/ teleport) snaps instead of sliding. Velocity-less (keyframed) objects that
jumped in discrete steps now glide between updates. `render_offset` /
`render_rot_offset` on `PhysicsInterp`; three unit tests
(continuity-then-convergence, region-scale snap, generic smoothing-alpha).
**Angular smoothing** (the third filed contributor) is subsumed here — the
rotation residual eases the per-update rotation snap.

**Update cadence vs. frame rate (second filed contributor).** Measured on aditi:
the sim streams a driven vehicle to us at only **~14 Hz, irregular** (14–250 ms
gaps), so extrapolation error per update was ~0.8 m (≈130 % of one update-step's
travel) — the visible jerk. The camera/interest `AgentUpdate` cadence was raised
from 2 Hz to ~45 Hz (`session.rs`, `CAMERA_INTEREST_MIN_PERIOD_SECS`, send-on-
movement) to match the reference (which sends up to 125 Hz); this did **not**
raise the object stream (the sim's rate is what it is) but is the correct,
reference-faithful behaviour and helps the interest list. The object's
world-space motion is left at **reference parity**: the reference viewer
extrapolates the same sparse stream with no interpolation buffer
(`LLViewerObject::interpolateLinearMotion`), so Firestorm shows the same world
jitter. Not pursuing snapshot-interpolation (would add latency and diverge from
the reference); the remaining `sPingInterpolate` parity gap is a separate
follow-up ([[viewer-object-ping-forward-interpolation]]). Region-crossing
prediction is already at LL parity (`clamp_prediction` / `region_cross_expire`).

**The actual smoothness win was the camera.** The Kart's seat sets a scripted
sit camera, and that path read the seat's frame-stale `GlobalTransform` — so the
vehicle wobbled a frame behind in the driver's view on every correction. Fixing
it to the current-frame seat pose locked the vehicle in view and was reported
"much smoother." Split out as [[viewer-sit-camera-vehicle-frame-lag]] (a sibling
of the seated-rider fix). The scripted speed-based zoom-out seen in Firestorm is
a further unimplemented mechanism,
[[viewer-scripted-followcam-llsetcameraparams]].
