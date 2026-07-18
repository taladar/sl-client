---
id: viewer-avatar-motion-render-smoothing
title: Smooth the avatar's rendered position (dead-reckoning jitter)
topic: viewer
status: done
origin: surfaced by the following third-person camera in the camera-system pass
---

Context: [context/viewer.md](../context/viewer.md).

The own avatar's rendered body **vibrates** with a high-frequency, ~cm
whole-body jitter, obvious under the new head-following third-person camera
([[viewer-camera-third-person-orbit]]) and invisible with the old detached
debug fly-camera.

**Original hypothesis (wrong):** the dead-reckoner (`crate::physics`
`drive_avatar_motion`, P31.4) snapping the render `Transform` to each terse
`ImprovedTerseObjectUpdate`. Ruled out by measurement: a standing avatar
receives essentially **no** updates, yet jitters the most, and the body-root
anchor is provably still.

**Actual cause:** an interaction between **avian** and the animation pose write.
`pose_avatar_skeletons` (P18.3) writes each joint's animated `GlobalTransform`
**directly** (bypassing local `Transform`, to keep Second Life's non-standard,
non-scale-accumulating skinning convention shear-free). avian, by default, runs
Bevy's **general** `Transform` → `GlobalTransform` propagation
(`propagate_ parent_transforms` + `sync_simple_transforms`) before it steps
physics — in its `FixedPostUpdate` schedule, i.e. inside `RunFixedMainLoop`,
**before** `Update`. That pass recomputes the avatar joints' globals from their
rest local transforms. It fires only on render frames that run a fixed step (3
of every 4 at 45 Hz vs a 60 Hz display — the frame-locked ~15 Hz beat the
diagnostics showed), so the joints flick to the rest pose on those frames.

The **rendered body is unaffected** — `pose_avatar_skeletons` writes last in
`PostUpdate`, after this pass and before render extraction (confirmed: the pose
read in `Last` is smoothly animated every frame). Only systems that read a joint
`GlobalTransform` in **`Update`** saw the clobbered rest pose: the
**third-person camera focus** (head joint) and the **foot-IK ground probe**. The
camera's aim point flicked ~9.5 cm between rest and animated head, shaking the
whole view — the "whole-body vibration". The old detached fly-camera never read
the avatar, so it was invisible.

**Fix:** disable avian's redundant pre-physics propagation
(`PhysicsTransformConfig { propagate_before_physics: false, .. }` in
`PhysicsPlugin::build`). The viewer has no dynamic bodies (physical prims are
kinematic movers we snap each frame; colliders are inert until Phase 32/34), so
that pass is dead work — Bevy's own `PostUpdate` propagation still keeps every
physics body's `GlobalTransform` current. This removes the clobber at its
source; the `Update` readers now see the stable animated pose.

Considered and rejected: writing joint local `Transform`s and letting Bevy
propagate. Second Life's skinning recurrence
(`BevySkeleton::deformed_world_matrices`) uses each joint's **own** local scale
in the world matrix and applies a parent's scale only to child **positions**,
never accumulating it into the child basis. Bevy's standard propagation
accumulates scale into the basis, so local-`Transform` writes would double
scales down the chain and **shear** on the non-uniform bone scale shape sliders
apply — the R11/R13 distortion the direct-`GlobalTransform` write exists to
avoid.

Follow-up filed: [[viewer-stand-foot-ik-knee-asymmetry]] (a separate,
non-jittering asymmetry seen while diagnosing this).
