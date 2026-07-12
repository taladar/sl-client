---
id: test-object-update-decode
title: receive and decode the object-update stream; count primitives
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 8 — Objects & scene graph `[both]`
---

Context: [context/test.md](../context/test.md).

Most cases need a rezzed (and some a scripted) object — see appendix for the
OAR / XEngine setup.

`object-update-decode` — receive and decode the object-update stream;
count primitives. `1av`. After the region handshake the simulator streams the
agent's interest list — full `ObjectUpdate`s, `ObjectUpdateCompressed`, and
`ObjectUpdateCached` digests (whose cache misses this client resolves with a
`RequestMultipleObjects`, so the full update — and its
[`Event::ObjectAdded`] — follows a round trip later). The case observes that
stream for a 20 s window and tallies the first sighting of every region-local
id ([`Event::ObjectAdded`]) by `PCode`: primitives, avatars, and other
(trees/grass/…), deduplicated by id. This is the first case that must be
**co-located with a fixed in-world object**, which the login default of
`"last"` cannot guarantee — so it introduces a general
`GridTest::start_location(grid)` hook (default `"last"`, threaded through
`context::login`/`connect_and_spawn`/`relogin` and stored on the `Session` so
a relogin lands the same place). This case forces the OpenSim **Default
Region** (`uri:Default Region&128&128&30`), which holds this workspace's
rezzed test object, and keeps `"last"` on Second Life (a named OpenSim region
is meaningless there). Needs a rezzed object in that region (appendix); the
scripted prim left in Default Region by Phase 9's #8 setup serves. No new
client code — the `Object`/`pcode`/`ObjectAdded` surface all already existed;
only the harness login hook plus the new case. Green on OpenSim: 1 primitive
(the test object) + 1 avatar (self) decoded, `first_object` ≈ 1 ms. On SL the
landing region's contents are uncontrolled — zero primitives in the window is
recorded `partial` rather than failed. `[both]`; the aditi run is deferred
with the batch (no aditi record this session).
