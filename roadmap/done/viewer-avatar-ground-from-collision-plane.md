---
id: viewer-avatar-ground-from-collision-plane
title: Avatar ground probe from the simulator's collision plane (drop the raycast)
topic: viewer
status: done
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

## Verified (2026-11-08) by automation on the fake grid, OpenSim and aditi

`tests/e2e_movement.rs` checks this through two new probe fields on the agent
readout: `foot_plane` (the plane the viewer holds for the own avatar) and
`feet` (each ankle as last posed, the ground the probe found under it, the
foot IK's weight, and whether a built-in stand is playing; the viewer
registers the reader in `automation_sources.rs`, fed by `OwnFeet`, which the
pose pass now records in `AvatarGround`).

- **Plane populated on both live grids.** After each walk the plane is a
  floor under the avatar's centre and never under the land. Both grids send
  `[0, 0, 0, 1]` while the avatar is airborne: an aditi login drops the
  avatar about 13 m, so the stage polls up to 30 s. SL lets an avatar stand
  on a 46° bank (`nz` 0.69, Mauve), so the floor test accepts `nz ≥ 0.5`.
- **Flat ground.** Fake grid, OpenSim and aditi: the foot IK is fully applied
  and both ankles stand 5 to 12 cm over their ground. The stage's band is
  2 to 20 cm, with the two ankles within 5 cm of each other.
- **Slope.** A new fake-grid stage starts the avatar on a ridge's 21° flank,
  facing along it: the planes carry the slope normal, the uphill foot's
  ground is 6 cm higher, and the ankles still agree within 5 cm. It runs on
  the fake grid only, because a live grid's terrain can't be shaped per test.
  To make it possible, the fake grid now honours a login's `uri:` start
  location: a named region at that x/y, on the ground there whatever height
  the start names (`login_arrival`; before this it always used the first
  region's default spot).
- **Animation overriders.** On aditi the test avatar wears an overrider:
  a few seconds after login the grid plays only uploaded stands. The foot IK
  plants the feet only under the built-in stands, like the reference's
  `LLKeyframeStandMotion`, so its weight is then 0. That is faithful, so the
  stage logs a warning and leaves the feet unchecked. On the fake-grid slope
  stage, a missing built-in stand is still a failure.
- **Not covered: a prim floor or a ramp.** It needs a live grid (the fake
  grid's controller has no prim collision), so it stays in
  [[test-e2e-sweep-live-grid]].

This is still **independent of** [[viewer-avatar-falls-through-ground]],
which uses the same plane for the body's floor.
