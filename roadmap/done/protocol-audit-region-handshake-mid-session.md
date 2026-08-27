---
id: protocol-audit-region-handshake-mid-session
title: A root RegionHandshake outside AwaitingHandshake is silently dropped
topic: protocol
status: done
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session/methods.rs:2798` wraps the **entire** `RegionHandshake`
arm — `send_region_handshake_reply`, `note_region_flags` and the
`Event::RegionInfoHandshake` push — inside
`if matches!(self.state, SessionState::AwaitingHandshake)`.

A simulator re-sends `RegionHandshake` while the session is `Active` on a region
restart, an estate change or a terrain-texture change. Nothing replies, so the
sim keeps retrying, and the region flags never refresh.

The **child** arm at `:1957` replies and updates flags unconditionally. The two
are hand-copied dispatchers that have drifted — see
[[protocol-audit-dispatch-child-drift]] for the structural fix that stops the
next one.

## Fixed (2026-08-27)

The state gate is gone: the root arm now replies, re-reads the region handle,
re-surfaces `Event::RegionInfoHandshake` and re-records the region flags for
every `RegionHandshake`, matching the child arm and the reference viewer
(`process_region_handshake` in `llworld.cpp` is ungated — it calls
`unpackRegionHandshake`, which always sends the reply).

Nothing else needed a guard, because the only once-only part of the arm was
already self-guarding: `complete_arrival` returns immediately unless the state
is still `AwaitingHandshake`, so the arrival transition — the `AgentUpdate` /
ping timers, the throttle re-advertise, `Active`, and
`RegionHandshakeComplete` / `RegionChanged` — still fires exactly once. The
existing teleport test that asserts a later handshake raises no second
`RegionChanged` now covers the ungated path.

Both downstream consumers of the refreshed event already treat it as an update
rather than a first sighting: `sl-viewer-world-scene`'s terrain re-learns the
composition, bumps the region revision and re-queues its patches (which is the
point of a terrain-texture change), and water re-inserts the region's height.

One test, driving an already-`Active` session through a second handshake that
renames the region and sets `BLOCK_FLY`: the reply goes out, the identity event
carries the new name and flags, `region_blocks_fly()` flips, and no second
`RegionHandshakeComplete` is emitted.
