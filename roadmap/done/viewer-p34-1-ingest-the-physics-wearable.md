---
id: viewer-p34-1
title: Ingest the physics wearable
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 34 — Avatar cloth & body physics
refs: [viewer-p31-12a, viewer-p34-2]
---

Context: [context/viewer.md](../context/viewer.md).

**Done.** The `WT_PHYSICS` wearable is now ingested end to end: a new pure
`sl-avatar::physics` module resolves an appearance into a `BodyPhysics` — the
six `LLPhysicsMotionController` motions (breast up-down / in-out / left-right,
belly up-down, butt up-down / left-right), each with its joint (`mChest` /
`mPelvis`), its motion-direction vector, its seven resolved spring-damper
settings (mass, gravity, drag, spring, gain, damping, max-effect), and the
`*_Driven` morph params its hidden controller drives.
`PhysicsMotionConfig::driven_weight` is the reference's `setParamValue` mapping
(a simulated position in `0..=1` → the driven param's weight, squeezed into the
`Max_Effect`-wide window centred on the user's own shape), so P34.2 only has to
integrate. Ingest happens off the same `ResolvedParams` the morph / skeletal
passes already build, and the result is kept per avatar in
`AvatarState::body_physics`.

Two things had to be discovered before the driven params could work at all:

- **Their morph targets exist in no `.llm`.** `Breast_Physics_UpDown_Driven` and
  friends are named by `avatar_lad.xml` but are absent from every base mesh; the
  reference viewer *manufactures* them while loading each part
  (`LLPolyMeshSharedData::loadMesh`), cloning a shape morph that already moves
  the right vertices: `Breast_Gravity` → up-down, `Breast_Female_Cleavage` →
  in-out (verbatim) and left-right (`clone_morph_param_cleavage`, 0.75 with the
  Y sign mirrored so both breasts sway the *same* way), and `Big_Belly_Torso` /
  `Big_Belly_Legs` / `skirt_belly` / `Small_Butt` → the belly and butt targets
  (`clone_morph_param_direction`, which replaces the source deltas with one
  constant displacement and keeps only *which* vertices move).
  `BaseMesh::from_bytes` now does the same.

- **They also displace collision volumes.** A `<param_morph>` may carry
  `<volume_morph>` children (`LEFT_PEC`, `BELLY`, `BUTT`, …); these are now
  parsed into `ParamEffect::Morph(Vec<VolumeMorph>)`. Since P17.2 the collision
  volumes *are* bindable joints, so this is the path by which a worn
  **rigged mesh** body bounces — a system-body morph target cannot reach it.
  (Only the physics ones are driven per frame; the ~30 *shape* params that also
  carry volume morphs are parsed but still unapplied — see [[viewer-p34-3]].)

The eight `*_Driven` params join `RUNTIME_MORPH_PARAMS`, so they are excluded
from the static bake and built as GPU morph targets instead — the bounce needs
no re-bake, exactly like the blink ([[viewer-p31-12a]]) and the hand poses.

Unit-tested (the three clone recipes; ingest of a motion's settings, driven
params and volume morphs from an appearance vector; the `Max_Effect` window,
including that a zero effect pins every driven param at the user's own shape).
**Live-verified against OpenSim**: the real `avatar_lad.xml` resolves to
`body physics for <agent>: 0 of 6 motion(s) active` — all six motions found and
configured, none switched on, because the test avatar wears no physics wearable
— and the body still shapes identically (`8 body part(s) + 160 joint(s)`), which
is the no-regression check that matters for moving those params out of the bake.
