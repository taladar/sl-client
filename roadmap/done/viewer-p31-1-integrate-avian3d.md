---
id: viewer-p31-1
title: Integrate avian3d
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

Object **and avatar** position is *entirely* simulator-authoritative — the
reference viewer never runs a client-side physics solve for their placement, and
**does no collision/wall prediction** (not even for the own avatar; the agent
body is the same `LLViewerObject` path). It only **dead-reckons** between
updates from the sim-sent linear velocity + acceleration
(`LLViewerObject::interpolateLinearMotion`, called from `idleUpdate`):
`new_pos = (vel + 0.5*(dt - PHYSICS_TIMESTEP)*accel) * dt`. No geometry is
consulted. The load-bearing protocol contract (verbatim from that function): the
sim *"will NOT send updates if the object continues normally on the path
predicted by the velocity and the acceleration (often gravity) sent to the
viewer"* — so silence means "prediction still holds", and a deviation (a wall, a
push, a settle, a script stop) is communicated by a **corrective update**, not
foreseen by the client. During the round-trip the viewer genuinely extrapolates
slightly *into* the wall and is then snapped back. There is **no** "settled"
flag; rest is inferred from a terse update carrying ~zero linear/angular
velocity.

Because unbounded extrapolation "walks off into infinity" (and sinks avatars
under the terrain / shoots them off on region crossings), the reference bounds
the dead-reckoning with a layered set of guards that P31.2's smoothing step
**must reproduce** rather than let a body free-run:

- **Circuit-health phase-out.** After `sPhaseOutUpdateInterpolationTime` (2 s)
  of silence *and* a blocked/stale circuit (`LLCircuitData::isBlocked` / no
  packets
  — checked on the whole circuit, since per-object silence is expected), a
  `phase_out` factor ramps `1.0 → 0.0`, multiplying both the position delta and
  velocity so the object **eases to a halt**; by `sMaxUpdateInterpolationTime`
  (3 s) prediction is fully off. The circuit gate is essential: it separates
  "quiet because the prediction is right" (keep going) from "quiet because the
  sim is lagging" (taper off).
- **Geometric clamps.** Each extrapolated step is clamped to a **ground floor**
  (avatars use a real land-height lookup `resolveLandHeightGlobal + 0.5*height`
  so a laggy avatar does not dead-reckon under the terrain), a
  **region-height ceiling**, and an **off-region edge clip**
  (`clipToVisibleRegions`) that, when the predicted position leaves into a void
  with no neighbour, clips to the edge,
  **zeros velocity + acceleration, and waits for a server update**.
- **Region-crossing cap.** A tighter `sMaxRegionCrossingInterpolationTime` (1 s)
  bounds interpolation across a border crossing (the classic "shot off across
  the region" source).

Implications for the implementation phases, to stay faithful:

- **Keep server-driven prims *and* avatars kinematic** — driven by
  `ObjectUpdate` transforms with, at most, this velocity+accel dead-reckoning
  (the "avian smooths between updates" half of P31.2), *including* the phase-out
  and clamps above. Do **not** integrate them as free dynamic bodies under the
  configured gravity: the moment a server object free-runs, the "sim considers
  it settled (and goes silent) but avian keeps simulating" divergence appears,
  with no incoming update to correct it — the one case the corrective-update
  model cannot close. avian's genuine *dynamic* bodies + the world `Gravity` are
  for **client-only** motion the sim never simulates (Phase 32 / 34), not for
  re-simulating server objects.
- **Client-only physics self-settles, so it has no authority conflict.** Flexi
  (Phase 32) and the avatar cloth/body params (Phase 34) are spring-damper
  systems driven by the sim/animation-authored motion; with zero input they
  relax to their rest morph rather than running away, so they cannot "un-settle"
  a settled avatar/prim the way a gravity-driven rigid body would.

The viewer today does **not** even dead-reckon — `objects.rs` snaps each
transform straight to the last reported `object.motion.position`. So adding the
Firestorm velocity extrapolation (with the guards above) is itself part of the
P31.2 "smooths between updates" work, not a prerequisite already in place.

**P31.1. Integrate `avian3d`.** Add the `avian3d` plugin: a physics
world with SL gravity, a fixed timestep, and coordinate bridging to the Y-up
scene. Foundation reused by Phase 32 and Phase 34. New workspace dependency.
**Done:** `avian3d` `0.7.0` (its `bevy ^0.19` requirement matches the
workspace Bevy) is added to `sl-client-bevy-viewer` only — like the render
materials and the other viewer-only simulations (sky / water / particles) the
physics world is a viewer rendering concern, not a protocol capability, so the
runtime-parity rule does not apply and `sl-client-tokio` gets nothing. A new
viewer `physics` module owns a `PhysicsPlugin` that adds
`PhysicsPlugins::default()` and configures the three foundation pieces: (a)
**gravity** — Second Life's `-9.8` m/s² Z-up world gravity (Firestorm
`llmath.h` `GRAVITY`, OpenSim `world_gravityz`) carried through the single
Second Life → Bevy basis change (`coords::sl_to_bevy_vec`), so avian's
`Gravity` resource points along Bevy `-Y`; (b) **fixed timestep** — avian runs
its schedule in `FixedPostUpdate` driven by Bevy's `Time<Fixed>`, pinned to
the simulator's target physics rate `SL_PHYSICS_HZ = 45`; (c) **time
dilation** —
avian's physics-clock *relative speed* (its own docs call it "time dilation")
is set each frame from the agent region's `RegionData.TimeDilation` (already
surfaced as `Event::TimeDilation`, folded per-region into a
`RegionTimeDilation` resource and looked up by
`SlIdentity::region_handle`), so client-side dynamics slow in lock-step with a
laden sim instead of drifting
ahead of it, defaulting to full speed while the region is unknown / healthy.
The physics world is empty (no bodies) until P31.2 gives server-flagged prims
rigid bodies, so there is no visible change yet. Verified: clean
build/clippy + 3 unit tests (gravity axis map, dilation clamp, bad-value
guard) and an OpenSim login smoke run (region handshake + clean quit, no
panics / avian / schedule errors).
