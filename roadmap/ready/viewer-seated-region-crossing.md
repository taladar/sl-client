---
id: viewer-seated-region-crossing
title: Seamless seated region crossing (keep sit-implied permissions)
topic: viewer
status: ready
origin: user question during viewer-sit-target-and-stand-button (2026-08-05)
refs: [viewer-sit-target-and-stand-button, viewer-sit-stand-actions]
---

Context: [context/viewer.md](../context/viewer.md).

When a **vehicle carrying a seated avatar crosses a region border**, Second Life
does a transient unsit / resit (largely simulator-side) as the object is handed
to the destination sim. That transient unsit tends to
**revoke the sit-implied script permissions** — `PERMISSION_TAKE_CONTROLS`,
`PERMISSION_TRACK_CAMERA`, `PERMISSION_TRIGGER_ANIMATION`, which sitting on a
scripted object grants without a dialog — so the destination sim's script has to
re-request them, and in the chaos of the crossing that re-request can error or
briefly break steering / camera / the sit animation. The user wants to avoid
that churn.

What already holds (do not re-do):

- The **session keeps the seat across a plain crossing**:
  `promote_child_to_root` leaves `SitState::Seated` intact and does **not** call
  `drop_inworld_grants` (only a real teleport / stand does), so `Session::seat`
  and the in-world script grants survive the border. `SlAgentParcel::seated_on`
  stays set (movement keeps routing the keys to the vehicle, not the avatar).
- **Placement** ([[viewer-sit-target-and-stand-button]]): the seated anchor is
  driven from its seat by the seat's *scoped* id, retargeted from each avatar
  `ObjectUpdate`'s `ParentID`. The vehicle's **region-local id changes** across
  the border (the destination sim's own id space), but a `ScopedObjectId` is
  `circuit + local id`, so the new region's ids never collide with the old
  region's — the retarget lands on the right entity. (The scripted sit
  **camera** instead keys on the vehicle's stable grid-wide `ObjectKey`, so it
  survives the local-id change directly.) The old region lingers as a *child*
  (its objects are not culled at the instant of crossing), so the seat retargets
  before the old vehicle's `ObjectRemoved` fires and the eager
  unseat-on-seat-removal does not misfire.

What this task is:

- **Preserve the sit-implied permissions through the crossing** so the
  destination script's re-request auto-grants (we are still seated) rather than
  dialoguing or erroring — the crux of the user's concern. Verify the
  script-permission subsystem auto-grants the controls / camera / animation trio
  while seated, and that a crossing does not transiently drop the grant on our
  side (it should not, per above — confirm and test).
- **Seamless placement across the border** — no visible unsit flicker or twitch
  as the seat retargets; interpolate / hold the seated pose across the handoff.
- Depends on the broader cross-region object handoff (neighbour → root
  promotion, new-circuit object streaming) being solid for the vehicle itself.

Reference (Firestorm, read-only): `LLAgent` sit / crossing handling, the vehicle
region-crossing path; `process_avatar_sit_response` re-grant flow.

## Progress (2026-08-07)

The **client-side invariant is pinned by a test**
(`sl-proto` `region_crossing_preserves_seat_and_inworld_grants`): a crossing
keeps `Session::seat()` **and** the sit-implied in-world script grants (the
controls / camera / animation trio), unlike a real teleport which clears both
(`teleport_clears_seat`, `teleport_drops_inworld_grants_keeps_attachment`). So
our mirror does not spuriously revoke the permissions mid-crossing — the
destination sim's re-grant lands cleanly.

Still **pending live verification on aditi**: OpenSim has
**no scripted vehicle** to sit on and carry across a border, so the end-to-end
seated-vehicle crossing (seamless placement, no permission churn) can only be
exercised on aditi — to be done after the rest of the teleport/crossing work is
solid. (A separate walk-crossing movement lockup surfaced live —
[[viewer-crossing-movement-locks-up]].)

## Parity-audit addendum (2026-08-19)

Addition from the audit of `fsregioncross.cpp`: the FS/Animats adaptive
extrapolation-time limit for ridden vehicles. We implement the LL
behaviour — a fixed 1 s crossing cap with zeroed acceleration
(`sl-client-bevy-viewer/src/physics.rs:948-1005`,
`REGION_CROSSING_CAP_SECS`). The reference low-pass filters the ridden
object's object-frame velocity/angular velocity and sets the
extrapolation limit to error-budget / current-deviation (settings
FSRegionCrossingSmoothingTime, FSRegionCrossingPositionErrorLimit,
FSRegionCrossingAngleErrorLimit), applied only to sat-upon objects that
have moved and are outside the 0..256 m region bounds — so smooth
vehicles extrapolate longer through a crossing and erratic ones stop
sooner.
