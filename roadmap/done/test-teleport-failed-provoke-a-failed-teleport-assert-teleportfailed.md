---
id: test-teleport-failed
title: provoke a failed teleport; assert TeleportFailed
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 12 — Teleport (state machine) `[both]`
---

Context: [context/test.md](../context/test.md).

`teleport-failed` — provoke a failed teleport; assert `TeleportFailed`.
`1av`. **Green on OpenSim.** Teleports to a region handle in the empty part of
the grid — grid `(2000,2000)`, far outside the local 2×2 block at
`(1000,1000)`–`(1001,1001)` — so no region occupies the handle. That forces
the *different-region* path in OpenSim's `EntityTransferModule`
(`TeleportAgentToDifferentRegion`), whose grid-service lookup returns no
region and answers `TeleportFailed` rather than a `TeleportStart` → arrival
sequence. The case waits for the first *terminal* teleport event — ignoring
any `TeleportStart`/`TeleportProgress` — and asserts it is
[`Event::TeleportFailed`], failing if instead an arrival
(`TeleportLocal`/`TeleportFinished`/`RegionChanged`) shows the teleport
unexpectedly succeeded; it also asserts the failure reason is non-empty.
Records the server-supplied reason (`"The region you tried to teleport to was
not found"` on the local grid), whether a structured `AlertInfo` accompanied
it (`false` on OpenSim), and the request-to-failure latency. **No new client
code** — the `Command::Teleport` / `Event::TeleportFailed` surface and
`RegionHandle::from_grid` already existed, and the session's `TeleportFailed`
handler was in place from earlier teleport work. `[both]`;
the aditi run is deferred with the batch (SL refuses a teleport to a
non-existent region the same way, with its own reason string and possibly an
`AlertInfo`).
