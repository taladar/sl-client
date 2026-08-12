---
id: viewer-avatar-falls-through-ground
title: Avatar falls through the ground (simulator reports a bouncing avatar position)
topic: viewer
status: in-progress
origin: observed during incremental-shadow-cull verification on aditi (2026-08-12)
refs:
  - viewer-avatar-ground-from-collision-plane
  - viewer-terrain-land-cache-on-disk
  - viewer-crossing-movement-locks-up
  - viewer-r23
---

Context: [context/viewer.md](../context/viewer.md).

Observed on aditi: the own avatar **falls through the terrain / a prim floor**
and **stays** under, rubberbanding up from each server position while walking,
until a teleport fixes it. It also **impedes walking** and **triggers the
fall / get-up animations** (sim-driven). Trigger: starts walking; happens on
plain terrain, not only near a prim. Distinct from [[viewer-r23]] (cosmetic
ankle-deep sink, in `done/`).

## Root cause — the SIMULATOR reports a bouncing avatar position (not dead-reckon)

Confirmed 2026-08-12 with a live aditi trace (a temporary
`SL_VIEWER_LOG_AVATAR_GROUND` diagnostic in `drive_avatar_motion`, since
reverted):

- The **authoritative** `ObjectUpdate` position for the own avatar bounces over
  a **~7 m range (Z 34.3 – 41.7 m)** while walking on ~flat ground, with a
  constant reported **`velocity.z = −7.94 m/s`** (bursts to −52.7). The client
  renders that faithfully → the avatar bounces / sinks.
- The terse decode is **correct**: `ImprovedTerseObjectUpdate` position is a
  full-precision `vector3()` (not quantized), and velocity dequantizes cleanly
  (0 → 0). So `34.3` and `−7.94` are genuinely what the sim sends, not a decode
  bug (`sl-proto/src/object_update/terse.rs`).
- It is **sim-side**: the fall / get-up animations are simulator-driven (the
  client locomotion fallback defers on SL — `locomotion.rs`), and walking is
  impeded — the sim itself believes the avatar is falling.

**Ruled out — client dead-reckoning.** Suppressing the avatar dead-reckoner's
vertical velocity entirely (`dr_vz = 0`) did **not** stop the sink, because the
*authoritative* positions themselves bounce. The whole dead-reckon line of
investigation (and the perf branch) is a dead end for this bug.

## Rejected attempts this session (reverted — do not retry)

- An authoritative-height **floor** with a "genuinely descending" gate: the
  sim's per-update height jitter flipped the gate frame-to-frame, so the floor
  toggled and **bounced** the avatar (a land / get-up / fall-down cycle without
  moving, delayed take-off). Worse than the bug.
- **Suppressing vertical dead-reckoning**: harmless but ineffective (the bounce
  is in the authoritative positions), and it adds a little lag to fast vertical
  motion. Reverted.

Both clamped the wrong layer: the bad data is in what the sim *sends*, upstream
of any client prediction.

## Fix (implemented — `physics.rs`, `terrain.rs`)

Render the avatar to the simulator's **authoritative ground**, so its bouncing /
low reported position can never drop it through the terrain:

- **`avatar_collision_floor`** (`physics.rs`): the Second Life capsule-centre Z
  floor at the avatar's `(x, y)` — the higher of the terrain land height and the
  simulator's **collision (foot) plane** (`AvatarMotion::collision_plane`,
  decoded from every avatar `ObjectUpdate` and previously ignored), each
  `+ 0.5·height`.
- **Applied in `drive_avatar_motion`** to the
  **authoritative *and* dead-reckoned** position (the old `avatar_ground_floor`
  only floored the *prediction*, so an authoritative low Z sailed through), and
  as a **hard clamp on the rendered Y** (no eased glide-up — the avatar never
  renders below the ground even mid-ease). The anchor's Bevy Y is a constant
  offset from the capsule Z, so the floor is applied by raising both by the same
  `Δ` — no root-drop maths.
- **`TerrainState.land_cache`** (`terrain.rs`): the last-known land patch per
  key, retained across a region teardown/rebuild so `land_height` keeps
  answering instead of returning `None` — because the **land** half of the floor
  is what catches a *large* sink (the plane tracks the avatar's own Z to within
  ~0.2 m, so it only catches small dips; the stable terrain height catches the
  metres).

Live-measured (aditi + OpenSim, `SL_VIEWER_LOG_AVATAR_GROUND`): the floor caught
every bounce while land was known (OpenSim ~0.05 m mean, aditi ~0.1 m), flight
to 67 m and back was never trapped (the plane tracks the feet, so the floor only
bites on a *dip below* it), and `floor_z` / `plane` were never `None`/absent
while a region was loaded.

Unit tests: `avatar_collision_floor` (higher of land/plane, plane-only when land
unknown, near-vertical plane ignored, neither → `None`); `land_height` cache
fallback (`terrain.rs`).

## Remaining

- **Login / cold-terrain window**: the in-memory `land_cache` is empty at login,
  so before terrain streams the floor has only the (avatar-tracking) plane and
  can still leak — the observer saw exactly this ("initially still fell through
  … after a teleport it was better", once terrain had loaded). Fixed by
  persisting the cache to disk — [[viewer-terrain-land-cache-on-disk]].
- The **foot-IK** ground probe was rewritten onto the same plane in
  [[viewer-avatar-ground-from-collision-plane]] (separate task, same data
  source).
- Still worth knowing (does **not** block the fix): does Firestorm bounce at the
  same spot? If our `AgentUpdate` is what destabilises the sim's avatar, fixing
  that would remove the bounce at the source rather than rendering over it.
