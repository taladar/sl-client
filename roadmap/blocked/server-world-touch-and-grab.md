---
id: server-world-touch-and-grab
title: Touch routing — a click on a prim reaches nothing
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
blocked_by: [protocol-sim-script-messages]
refs: [server-lsl-lib-detection-sensors, test-fake-grid-lsl-offline-cases,
  gridspec-touch-grab]
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

**Measured (2026-10-12, [[gridspec-touch-grab]],
`book/src/gridspec/building.md` § Touch and grab)** — build to this, per
flavour, where it differs from the rules above:

- The repeating `touch` is the region's doing **on Second Life only**
  (22.5 a second while a grab is open, and once for a bare click). OpenSim
  raises one `touch` for each `ObjectGrabUpdate` and none otherwise.
- A second `ObjectGrab` with a grab open is ignored on Second Life and a
  second `touch_start` on OpenSim; an update with no grab opens a touch on
  Second Life; a degrab with no grab is a `touch_end` on both.
- `llPassTouches`: OpenSim passes for any value but 0, so `PASS_NEVER`
  passes there. On Second Life a child with no handler under `PASS_NEVER`
  tells nobody.
- The detected block is the viewer's `SurfaceInfo` verbatim on both, of
  the latest message of the touch on Second Life; `llDetectedGrab` and
  `llDetectedType` differ, as tabled.
- A child of a root that takes touches carries `FLAGS_HANDLE_TOUCH` on
  Second Life and not on OpenSim.
- The drag half is done for a prim that is not physical
  (`sl-fake-grid/src/grabs.rs`), which stays where it is when its flags
  say a script takes touches — the script engine has to keep those flags
  true.

The census's script legs are live-only until this lands: enrol them on the
fake flavours (drop the `is_fake` early return in
`sl-conformance/src/cases/object_touch_grab.rs`) as the acceptance.

Acceptance: the `object-touch-grab` conformance case runs its script legs
against the fake grid — it is already in `fake::OFFLINE_CASES`, but stops at
the unscripted legs there and holds the fake flavours to `MEASURED` only, not
`MEASURED_LIVE` (audit 2026-10-15) — and a unit test shows a held grab
producing exactly one `touch_start`, N `touch` and one `touch_end` over
N+2 ticks.
