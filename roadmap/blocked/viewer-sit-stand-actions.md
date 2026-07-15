---
id: viewer-sit-stand-actions
title: Sit / stand actions
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-sit-stand
blocked_by: [viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

Sitting and standing: sit on an object (click / context "Sit Here"), ground-sit,
and stand up — driving the sit-target placement, the sit animation, and the
camera adjustment, then releasing on stand. Sitting on an object needs a
**picked object** ([[viewer-object-selection-core]]). The sit request is
permission- and target-aware (scripted sit targets, unsit on teleport).

Sit state is already modelled (`SitState` in `sl-proto`); this task is the
viewer action + UI (the stand button, the sit context action) and the pose /
camera handling.

Reference (Firestorm, read-only): `llagent` (sit / stand handling),
`llmoveview` (stand button), `llviewermenu` (sit context action); messages
`AgentRequestSit` / `AgentSit` / `AgentSitOnGround` and the `AvatarSitResponse`
reply.

Builds on: `SitState` (`sl-proto`), the animation playback (`animations.rs`),
and the camera system.
