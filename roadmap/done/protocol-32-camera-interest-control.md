---
id: protocol-32
title: Camera & interest control
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**32. Camera & interest control — real `AgentUpdate` camera fields · 3 pts. ✅
Done. (extends #3; was blocked on #16, Tier C.)** Item #3 noted the camera
"stays at region centre — true camera control waits on position tracking from
the object/scene graph (#16)." With #16 done, the `AgentUpdate` camera position
and at/left/up axes are now a real, caller-set viewpoint. A new `Camera` value
type (`sl-proto`) holds the eye `center` and the orthonormal `at`/`left`/`up`
basis, with a `Camera::looking_at(eye, target)` helper that derives the basis
with the world-up vector exactly as the reference viewer's
`LLCoordFrame::lookAt` does (`left = up × at`, `up = at × left`), plus
`Camera::region_center` (the historic default). `Session::set_camera` persists
it on the session and sends an immediate `AgentUpdate` on the root **and** every
child circuit; the viewpoint is then re-sent on every keep-alive (root and
neighbours) and survives region changes, exactly like #3's controls and #15's
throttle, so the simulator's interest list — and thus the per-category bandwidth
(#15) — follows where the agent looks rather than the region origin. The
previously-hardcoded region-centre camera in the `AgentUpdate` builder became
the `Camera::region_center` default, so behaviour is unchanged until a client
calls `set_camera`. Draw distance keeps its existing separate surface
(`Session::set_draw_distance`, the `AgentUpdate` `far` field). Wired as
`Command`/`SlCommand::SetCamera(Camera)` through both runtimes (re-exporting
`Camera`). Covered by four unit tests (the `looking_at` right-handed
orthonormal-basis construction, the straight-down degenerate fallback, the
region-centre default matching the legacy viewpoint, and a `lifecycle.rs`
`set_camera` test asserting the `AgentUpdate` carries the camera position/axes
and persists them on the next keep-alive). *Live-verified against the local
OpenSim via the `tokio_login_hold_logout` example: a `SetCamera` looking from
above the region centre toward the north-east ground round-tripped on one login
(a real orthonormal basis `at≈(0.65,0.65,−0.40)`, `left≈(−0.71,0.71,0)`,
`up≈(0.29,0.29,0.91)`), re-sent on each keep-alive across a 12 s hold, with a
clean login→logout lifecycle and no protocol error. Test: local OpenSim.*
