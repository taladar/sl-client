---
id: viewer-stand-foot-ik-knee-asymmetry
title: Standing foot-IK bends the two knees by different amounts on flat ground
topic: viewer
status: wont-do
origin: surfaced while diagnosing [[viewer-avatar-motion-render-smoothing]]
---

Context: [context/viewer.md](../context/viewer.md).

While diagnosing the standing-avatar jitter
([[viewer-avatar-motion-render-smoothing]]) with
`SL_VIEWER_LOG_LOCOMOTION_IK=1`, the own avatar standing still on **flat**
ground (OpenSim Default Region spawn) logged an **asymmetric** knee bend:

```text
walking=false standing=true ik_w=1.00 ground=(true,true,true)
disp=(+0.000,+0.000) slope=0.0deg knee=(5.6,14.0)deg
```

The values are *stable* frame to frame (so the solve is not oscillating — this
is not the jitter), but the two legs settle at visibly different knee bends
(~5.6° left vs ~14.0° right) even though the ground is flat, the slope is 0°,
and the foot displacement is zero on both feet. A symmetric stand on flat
ground should bend both knees equally.

The foot IK is fully engaged (`ik_w=1.00`) and the ground displacement it is
solving against is zero, so the asymmetry is not coming from the ground input.
Suspects to investigate: the L/R leg IK pole vectors / rest chains in
`locomotion_ik::apply`, an asymmetry in the resolved [`LegJoints`] or the
`clamp_goal_to_reach` back-off, or an asymmetric rest pose in the stand
animation the IK is correcting from. Reproduce with
`SL_VIEWER_LOG_LOCOMOTION_IK=1` on OpenSim, standing on flat terrain, and read
the `knee=(L,R)` column.

**Resolved not-a-bug (2026-07-22): the asymmetry is the authored pose of the
default "stand" animation, passed through faithfully.**

The last suspect on the list was the right one. Decoding the default `stand`
animation asset (`2408fe9e-df1d-1d7d-f4ff-1384fa7b350f` — OpenSim ships it in
`bin/assets/AnimationsAssetSet/`, and it is byte-identical to the copy our
viewer fetched into its animcache during the live repro) and computing each
knee's authored bend from its keyframe quaternions against the
`avatar_skeleton.xml` leg-bone offsets gives:

- left knee: **5.6°** at both keyframes,
- right knee: **14.0°** at both keyframes,

matching the logged `knee=(5.6,14.0)` **to the decimal**. The default stand is
a contrapposto pose (weight on the left leg, right knee relaxed) — a symmetric
stand was never what the animation authors.

This also *confirms* the foot IK's flat-ground invariant rather than
implicating it: at zero displacement the solver's law-of-cosines bend delta is
exactly zero (the goal is the current ankle), and the remaining plane-roll is a
rigid rotation of the whole chain, which cannot change the thigh–shin angle —
so the post-IK log reproduces the authored bends bit-for-bit. Firestorm plays
the same animation through the same `LLKeyframeStandMotion` machinery and
shows the same contrapposto. Nothing to fix.
