---
id: server-world-touch-and-grab
title: Touch routing — a click on a prim reaches nothing
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
blocked_by: [protocol-sim-script-messages]
refs: [server-lsl-lib-detection-sensors, test-fake-grid-lsl-offline-cases]
---

Context: [context/lsl.md](../context/lsl.md).

Once `ObjectGrab` / `ObjectGrabUpdate` / `ObjectDeGrab` arrive as typed
events ([[protocol-sim-script-messages]]), the region has to turn them
into the touch model a script sees — which is more than "call
`touch_start`".

The rules that content depends on:

- **The three events are a sequence, not three independent ones.** A
  left-click is `touch_start` once, then `touch` **every tick the button
  is held**, then `touch_end` once. A viewer sends one `ObjectGrab`, a
  stream of `ObjectGrabUpdate`s and one `ObjectDeGrab`; the repeating
  `touch` is the *region's* doing, on its own heartbeat
  ([[server-world-heartbeat]]), not one per update message.
- **They go to the root prim's scripts by default**, and to a child
  prim's own scripts as well, with `llDetectedLinkNumber` naming the
  link that was actually clicked. That needs
  [[server-world-link-sets]] to be meaningful.
- **`PASS_ALWAYS` / `PASS_NEVER` / `PASS_IF_NOT_HANDLED`
  (`llPassTouches`)** decides whether a child prim's touch also reaches
  the root. Implement it or state the default and leave a test asserting
  the default.
- **The detected parameter block** is filled from the grab: position,
  the `SurfaceInfo`'s UV (`llDetectedTouchUV`), normal
  (`llDetectedTouchNormal`), binormal, `ST` coordinate
  (`llDetectedTouchST`), face index (`llDetectedTouchFace`) and the
  grab offset. This block is shared with collision and sensor detection
  and belongs to [[server-lsl-lib-detection-sensors]]; this task's job is
  to *populate* it correctly from the wire data.
- **A touch on a non-scripted prim is not an error** and produces
  nothing, and a touch on a prim whose `click_action` is not
  `CLICK_ACTION_TOUCH` still fires the events — the click action is a
  viewer-side cursor hint, not a server-side filter. Worth a test,
  because assuming otherwise is the obvious mistake.

The grab path is also the drag path: `ObjectGrabUpdate` on a physical
prim is how a resident throws something. That half belongs to
[[server-world-collision-and-physics]]; this task should decode and
route it, and may leave the force application to that one.

Acceptance: the `object-touch-grab` conformance case runs against the
fake grid (it is OpenSim-only today) and a unit test shows a held grab
producing exactly one `touch_start`, N `touch` and one `touch_end` over
N+2 ticks.
