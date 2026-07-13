---
id: viewer-p31-12
title: Head & eye look-at tracking (LLHeadRotMotion / LLEyeMotion)
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
refs: [viewer-p31-12a, viewer-p31-12b]
---

Context: [context/viewer.md](../context/viewer.md).

**Done (head & eye rotation only).** The look-at target sourcing (own avatar
from the fly-camera, others from the sim-relayed `ViewerEffect` look-at),
`LLHeadRotMotion` head/neck/torso lag, and the `LLEyeMotion` eye aim + saccades
/ look-away jitter landed in `crate::look_at`. The **blink** was split out —
it drives the `Blink_Left` / `Blink_Right` morph visual-params per frame, which
needs a per-frame visual-param morph pipeline the appearance bake lacks:
[[viewer-p31-12a]] (the pipeline prerequisite) and [[viewer-p31-12b]] (the blink
itself).

**P31.12. Head & eye look-at tracking (`LLHeadRotMotion` /
`LLEyeMotion`).** The always-on adjusters split out of P31.8 that need a world
**look-at target**, which the viewer does not yet track. `LLHeadRotMotion`
turns the head toward the target and lags the neck (`NECK_LAG`) and torso
(`TORSO_LAG`) behind it, constrained to `HEAD_ROTATION_CONSTRAINT`; with no
target it faces the root forward (rest), so it is a near no-op until a target
exists. `LLEyeMotion` aims the eyes at the target with vergence, layers random
saccades / look-away jitter on top (bounded by `EYE_ROT_LIMIT_ANGLE`), and
blinks by driving the `Blink_Left` / `Blink_Right` **morph visual-params**
(needs runtime per-frame visual-param morphs, which the appearance pipeline
bakes once — an extra prerequisite). First provide the look-at target: for the
own avatar derive it from the camera / cursor focus; for others from the
sim-relayed `LookAt` (the `ViewerEffect` look-at the P11-era data carries).
Then port head/neck/torso lag and the eye aim + saccades. Reference:
`llheadrotmotion.cpp` (`LLHeadRotMotion` / `LLEyeMotion`).
