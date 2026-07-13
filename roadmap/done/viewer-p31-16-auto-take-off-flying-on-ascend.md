---
id: viewer-p31-16
title: Auto-take-off flying on ascend while standing
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model
  (read before P31.2); follow-up to viewer-p31-11
---

Context: [context/viewer.md](../context/viewer.md).

**P31.16. Auto-take-off flying on ascend.** The mirror of viewer-p31-11
(auto-stop on landing): in the reference viewer, holding the ascend key while
standing on the ground starts flight — `agent_jump` in `llviewerinput.cpp`
sends a jump/ascend immediately, but once the key has been held past
`FLY_TIME` (0.5 s) / `FLY_FRAMES` (4 frames) with `AutomaticFly` on it calls
`LLAgent::setFlying(true)`, which only takes off if `LLAgent::canFly()` holds
(a tap just jumps; a hold takes off). Flight is refused when the region or the
agent's parcel disallows flying completely.

Our viewer (`movement.rs`, **PageUp** = ascend): while not flying and grounded
(reuse `AvatarMotion::at_ground_floor`), hold PageUp past the take-off threshold
to set `AvatarControls::flying = true`, gated on the fly permission that
protocol-66 surfaces (`SlAgentParcel::can_fly`). Holding the ascend key also
naturally suppresses the P31.11 auto-land (it requires no ascend key), so a
take-off is not immediately undone. Keep the manual **F** toggle. Unit-test the
pure take-off decision (hold duration threshold, grounded, not already flying,
permission).
