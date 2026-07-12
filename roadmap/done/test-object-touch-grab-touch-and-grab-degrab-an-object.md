---
id: test-object-touch-grab
title: touch and grab/degrab an object
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 8 — Objects & scene graph `[both]`
---

Context: [context/test.md](../context/test.md).

`object-touch-grab` — touch and grab/degrab an object. `1av`. The
two ways a viewer physically interacts with a prim, both exercised against
one primitive: a **touch** (left-click) via [`Command::TouchObject`] (an
`ObjectGrab` immediately followed by an `ObjectDeGrab`, the click that fires a
script's `touch_start`/`touch_end`), and a full **press-drag-release** —
[`Command::GrabObject`] → [`Command::GrabObjectUpdate`] (keyed by the
persistent object id, not the region-local id) → [`Command::DegrabObject`].
All four are unacknowledged at the application layer — the simulator sends no
reply a viewer waits on (any visible effect is a *script's* reaction, which a
stock prim need not have) — so, like `draw-distance`'s unreliable
`AgentUpdate`, "no error" is read from the circuit staying healthy: a
keep-alive ping still round-tripping after the interaction. The messages are
reliable, so a failure to encode or enqueue any of them propagates from `send`
and fails the case first. Reuses the object-find and `start_location`
machinery of `object-properties`/`object-update-decode` (Default Region on
OpenSim; a no-primitive region fails there and records `partial` on SL). No
new client code — the
`TouchObject`/`GrabObject`/`GrabObjectUpdate`/`DegrabObject` surface all
existed; only the new case. Green on OpenSim against the Phase 9 scripted
prim: touch + grab cycle sent, ping RTT ≈ 0.6 ms loopback. `[both]`; the aditi
run is deferred with the batch (no aditi record this session).
