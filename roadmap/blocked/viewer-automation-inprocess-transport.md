---
id: viewer-automation-inprocess-transport
title: In-process transport — the same requests against Apps in the test process
topic: viewer
status: blocked
origin: viewer automation design (2026-09-28)
points: 5
blocked_by: [viewer-automation-executor, viewer-automation-per-app-state,
  viewer-automation-windowless-mode]
refs: [viewer-automation-driver]
---

Context: [context/automation.md](../context/automation.md).

The second backend: viewers as Apps inside the test process, built by
`ViewerAppBuilder` windowless, driven by the same `Request`s. Faster to
start, debuggable in one process, and runnable under a paused clock.

## Wanted

- A transport that owns one or more Apps and steps them (round-robin, with
  the `FRAME_PAUSE` courtesy the full-stack tier uses so network threads
  progress) while requests are outstanding.
- Same `Request` / `Response` values as the remote transport, handed to the
  executor directly.
- Several viewers in one process, each its own App with its own login.

Acceptance: the executor's end-to-end test passes unchanged through this
transport; two Apps log into one fake grid and both answer requests.
