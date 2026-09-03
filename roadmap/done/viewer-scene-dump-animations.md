---
id: viewer-scene-dump-animations
title: A scene dump says what each avatar is playing, and where its clock is
topic: viewer
status: done
origin: a pose difference in the cross-check frames that turned out to be phase
  (2026-09-03)
points: 3
refs: [viewer-scene-dump, test-firestorm-crosscheck-report]
---

Context: [context/testing.md](../context/testing.md).

Two cross-check runs looked like they had found a pose divergence: the
catalogue's NPCs were posed differently in the two viewers' frames — one
arms-out with a twisted torso, the other arms-down and in profile — and
which viewer showed which changed between runs. Both scene dumps agreed
field for field on those very runs, because a dump said where an avatar was
and not what it was doing.

It was **phase**. The fixture animation is a 2 s loop (a 60° yaw of
`mChest`, out and back) and the harness captures a frame every 0.5 s, so
any two frames catch two arbitrary points in the loop, and neither viewer
starts playing at the same moment as the other. Nothing was wrong, and
nothing in the document could say so.

So each avatar entry now carries the animations it is playing:

- **in the order the viewer applies them** — most recently activated first,
  which is the order the reference's `LLMotionController` keeps
  `mActiveMotions` in (it front-inserts on activation) and the order our own
  per-joint blend breaks a priority tie by. Order is half of what decides
  which motion owns a joint;
- `priority`, which is the other half;
- `sequence`, the simulator's per-avatar number, present only for an
  animation the simulator asked for — a motion a viewer started itself
  carries no number of the simulator's;
- `time`, seconds since that viewer started playing, and `loop_time`, the
  same number wrapped into the motion's own duration. **Only the second is
  comparable between two viewers**, and it is the "which frame" the body
  was drawn at;
- `duration`, `looping` and `stopping`, each absent rather than invented
  when the asset has not arrived.

Both viewers write it. The reference half is `buildAnimations` in
`fstestscenedump.cpp` (in the fork), which needed one accessor —
`LLMotion::getActivationTimestamp` — because `flushAllMotions` computes the
same `mAnimTime - mActivationTimestamp` difference from inside the class.

**The two viewers list different sets, and that is not a divergence.** The
reference starts default motions on every avatar — head rotation, eye, body
noise, breathing, physics, hand pose, pelvis fix — which this viewer
implements as adjusters rather than as motions, so they appear only on that
side. What is worth comparing is the animations the *simulator* named:
whether both viewers play them, and where each one's clock has got to.
[[test-firestorm-crosscheck-report]] must expect the extra motions rather
than rank them.

The lesson is the one that nearly cost a wrong bug report: **a contact
sheet of an animated subject compares two phases, not two viewers.** Rank a
scene with a moving avatar by pixels and a reader will believe whatever the
sampling happened to catch — unless the document beside it says where each
animation's clock had reached.

Verified against Firestorm on the catalogue scene (2026-09-03). Both
viewers report the fixture's twist identically — sequence 1, duration 2 s,
looping, priority 4 — on both NPCs and on the animesh's control avatar, and
the phase the frames had been arguing about is now a number: `loop_time`
0.55 s here against the loop boundary there, out of the same two-second
motion. The reference lists six or seven procedural motions per avatar that
this viewer does not, each with no duration, which is the asymmetry above
showing up exactly where it was predicted.

One trap worth writing down, because it cost a run: `make firestorm-bin`
does **not** refresh the packaged tree, and the launcher runs
`packaged/bin/do-not-directly-run-firestorm-bin`. A run against a stale copy
reports the *old* schema — an absent `animations` section reads as "the
reference does not emit this" rather than "you compared yesterday's
binary".
