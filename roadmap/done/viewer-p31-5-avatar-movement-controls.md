---
id: viewer-p31-5
title: Avatar movement controls
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.5. Avatar movement controls.** A prerequisite that rode along
with P31.4: a fly-camera-only viewer never moves the own avatar, so
avatar dead-reckoning could not be *observed* (the stationary own avatar
reseeds to truth every frame). Second Life avatar motion is entirely
simulator-authoritative — the client advertises *intent* in `AgentUpdate`
(`ControlFlags` + the body facing the walk direction follows) and the sim
moves the avatar and streams it back — so this drives that intent from
the keyboard, which is exactly what feeds the P31.4 dead-reckoner. A new
`movement.rs` (viewer-only) reads keys that do **not** clash with the WASD
and mouse fly-camera: **↑ / ↓** walk forward / back (`AT_POS` / `AT_NEG`),
**← / →** turn the client-tracked heading (sent as the `AgentUpdate` body
rotation the walk follows, seeded once from the own avatar's reported
facing so the first step does not snap), **PageUp / PageDown** ascend /
descend, **F** toggle fly, **Shift + ↑ / ↓** run (`FAST_AT`). No stop key
— the flag set is recomputed from the held keys each frame, so releasing a
key drops its flag. It emits a command only when the intent *changes* (a
`SetControls` on a flag change, a throttled `SetRotation` while turning),
relying on the session's keep-alive re-send of the held controls. The
whole movement / rotation stack (`Command::SetControls` / `SetRotation`,
`ControlFlags`) already existed end-to-end through both runtimes, so this
was a viewer-only addition (module + registration). Verified in the same
live run above.
