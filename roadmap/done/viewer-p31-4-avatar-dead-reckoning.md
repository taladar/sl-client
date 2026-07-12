---
id: viewer-p31-4
title: Avatar dead-reckoning
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.4. Avatar dead-reckoning.** Extend the P31.2
`interpolateLinearMotion` port to the own and other avatars (the `avatars.rs`
path, not the object path), so a laggy avatar dead-reckons from its sim-sent
velocity / acceleration with the same phase-out and clamps. Avatars use the
stricter **ground floor** the reference viewer applies to them
(`resolveLandHeightGlobal + 0.5*height` via `TerrainState::land_height`) so a
laggy avatar does not sink under the terrain — the one guard P31.2 left
permissive for objects. Keep avatars **kinematic** (sim-authoritative), like
the objects. **Done:** the P31.2 object dead-reckoner was refactored so
its extrapolation core is shared — a new `MotionState` (the evolving
predicted pose + motion, all in Second Life space) plus an
`advance_motion` step (the dead-reckon + geometric-clamp + angular-spin,
taking a caller-supplied ground floor) now back **both** paths;
`PhysicsInterp` was reshaped to hold a `MotionState`, unchanged in
behaviour. On the avatar side, `apply_object` (`avatars.rs`) stamps each
full-object avatar's anchor with a new `AvatarMotion` (change-detected,
re-inserted every update — a rigged body root carries the object
rotation, a placeholder sphere does not), and a new `drive_avatar_motion`
system dead-reckons it with the **stricter avatar ground floor**
(`avatar_ground_floor` = `land + 0.5*height`, vs. the object floor's
permissive `land - radius`). Because the anchor's Bevy `Transform` also
carries the pelvis / shoe vertical render offset (owned by `apply_object`
/ the appearance path), the driver moves it by the SL-space position
*delta* (the basis change is linear, so it converts directly) rather than
recomputing it absolutely, leaving that offset intact; on an
authoritative update `apply_object` has already snapped the anchor to
truth, so the driver only reseeds. Kept viewer-only (no runtime-parity
obligation). Eight new unit tests (avatar floor, the shared
`advance_motion` dead-reckoning + floor clamp + the still-body no-op, plus
the movement-control helpers below) on top of the P31.2 suite. Verified
live on OpenSim: the own rigged avatar was seeded (`avatar … →
dead-reckoned (height 1.90 m, rotates true)`) and — driven by the new
movement controls (P31.5) — walked / turned / flew **smoothly** between
the sim's updates (user-confirmed on screen), with a clean session and no
panics / avian / schedule errors. **Follow-up noted:** the avatar does
not yet *animate* while it moves → **P31.6**.
