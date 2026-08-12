---
id: viewer-own-avatar-facing-drifts-idle
title: Own avatar forward direction drifts every few seconds while idle
topic: viewer
status: bugs
origin: observed live on aditi during the collision-plane ground-floor work (2026-08-12)
refs:
  - viewer-avatar-falls-through-ground
---

Context: [context/viewer.md](../context/viewer.md).

Observed live on aditi: the **own** avatar's **forward/facing direction changes
slightly every few seconds** without any turn key pressed (the avatar was not
being actively steered). Small, periodic yaw drift.

Almost certainly **not** related to the collision-plane ground-floor change that
was in flight when this was noticed — that edit only touches the *vertical*
(`AvatarMotion` `position.z`, the anchor's target/rendered `y`); the avatar's
heading comes from `AvatarMotion::rotation` eased through
`smoothed_rotation` (`physics.rs`), which it does not touch.

## Likely suspects (to check)

- The simulator re-broadcasting a slightly jittery `ObjectUpdate` rotation for
  the own avatar (terse-update quantised rotation echoes), which the P31.7
  rotation ease then glides toward — a periodic micro-yaw as the coarse
  quantised value flips between adjacent codes.
- The movement/heading seed (`movement.rs`) re-seeding the walk heading from the
  reported yaw when idle.
- A look-at / camera-driven `SetRotation` being sent and echoed back.

## To pin down on the next repro

- Is it the **rendered** rotation only, or does the authoritative
  `AvatarMotion::rotation` itself change (log both)?
- Does it correlate with `ObjectUpdate` arrivals (every few seconds ≈ the terse
  update cadence for a still avatar)?
- Does it happen for **other** avatars too, or only the own?
- Firestorm at the same spot — does the reference show the same micro-drift
  (→ sim-side quantisation) or is it stable (→ our decode / ease)?
