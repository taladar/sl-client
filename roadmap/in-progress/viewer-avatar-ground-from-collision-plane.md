---
id: viewer-avatar-ground-from-collision-plane
title: Avatar ground probe from the simulator's collision plane (drop the raycast)
topic: viewer
status: in-progress
origin: falls-through-ground investigation surfaced that we ignore the foot plane (2026-08-12)
refs:
  - viewer-perf-avatar-ground-probe
  - viewer-avatar-falls-through-ground
---

Context: [context/viewer.md](../context/viewer.md).

Replace the avatar foot-IK **ground probe**'s whole-scene `MeshRayCast` with the
simulator's authoritative **collision (foot) plane**, combined with the terrain
land height — exactly what the reference viewer's
`LLWorld::resolveStepHeightGlobal` (`getGround`) does. The reference does
**not** raycast object geometry for the avatar ground: the sim has already done
the avatar-vs-world collision and reports the resulting surface as `mFootPlane`
on every avatar `ObjectUpdate`. We decode that plane (`sl-proto`
`ObjectMotion::collision_plane`) but until now threw it away and re-derived the
ground with three per-avatar, per-frame rays.

Motivation: **server-consistency** (the plane already accounts for the prim the
avatar stands on, so a prim floor above terrain grounds natively — no client
raycast, no divergence) and **performance** (removes the whole-scene raycast
whose cost tracked scene-mesh count; see [[viewer-perf-avatar-ground-probe]],
now superseded).

## Implemented (2026-08-12)

- **`AvatarMotion` carries the plane** (`physics.rs`): `from_object` copies
  `object.motion.collision_plane`; a `collision_plane()` accessor exposes it.
- **`ground.rs` rewritten**: `resolve_ground(sample, plane, land_sl_z, offset)`
  takes the higher of the plane's Z at the sample's horizontal and the land
  height, with the plane's (Bevy-world) normal when it wins; `None` when neither
  is known or the surface is outside the `±1 m` probe reach (airborne — same
  band the old raycast used). The plane is cached per avatar
  (`AvatarGround::planes`) so a plane-less compressed update holds the last good
  plane, as the reference holds `mFootPlane`. Seated avatars are still skipped.
  `AgentGround` / `GroundHit` are unchanged, so `locomotion_ik` and the pose
  pass are untouched.
- **Removed**: the `MeshRayCast`, the avian `SpatialQuery` fast path and its
  `SL_VIEWER_GROUND_PROBE_SPATIAL` toggle, the per-patch terrain trimesh
  collider (`terrain.rs` — only built for that path), and the
  `accept`/`avatar_roots` parent-walk filter.
- **Unit tests** (`ground.rs`): flat plane; prim plane above land wins;
  land-only fallback; airborne (far surface) → `None`; no-plane-no-land →
  `None`; sloped plane projects + tilts the normal; region-offset shift.

## Faithfulness notes

- The single plane is evaluated **per foot** (root + each ankle projected onto
  it), which is what the reference does — `getGround` is called once per point.
  One planar surface under the whole avatar; a foot off a ledge onto a
  *different* surface is not distinguished, exactly as in the reference (it has
  one `mFootPlane`).
- On sloped terrain the sim reports the slope **as the foot plane**, so the
  plane branch carries the real normal for ankle roll; the bare-land branch (up
  normal) is only the no-plane/airborne fallback where no foot rolls.

## Remaining — live-verify on BOTH grids (they may differ)

- **aditi** and **OpenSim** (the two may send the plane differently — OpenSim's
  `CreateImprovedTerseBlock` vs SL): walk across flat terrain, a slope, and onto
  a prim floor/ramp; feet must stay planted with correct slope roll, no
  punch-through, no false-airborne bank. Confirm the plane is populated (a
  plane-less grid would fall back to land-only — acceptable but note it).
- This is **independent of** [[viewer-avatar-falls-through-ground]] (the
  body/root bounce is the sim's authoritative *position*, not the ground probe).
  If the live plane turns out stable while that position bounces, the same plane
  is also the natural sim-authoritative floor for that bug — a follow-up, not
  this task.
