---
id: viewer-automation-remote-transport
title: Remote transport — automation over a private Unix socket
topic: viewer
status: ready
origin: viewer automation design (2026-09-28)
points: 5
blocked_by: [viewer-automation-executor]
refs: [viewer-automation-driver, viewer-automation-ctl-cli]
---

Context: [context/automation.md](../context/automation.md).

The real binary is driven from outside its process: by `sl-e2e`, by the
CLI, by an agent. The transport carries `sl-automation-proto` requests to
the executor and responses and event streams back.

## Why not `bevy_remote`

It was the first candidate (it is in the Bevy fork, with custom and watching
methods) and was rejected on review (2026-09-28):

- `RemotePlugin::empty()` is private, so `Default` always registers the
  built-in world-writing methods (insert / remove / spawn / despawn) on an
  **unauthenticated TCP port** — any local process could rewrite the
  viewer's world;
- its HTTP server starts a second listener for the render sub-app on the
  **fixed port 15703**, so two viewer processes collide, and bind errors are
  lost in a detached task (`http.rs`), which hides an error;
- bound on port 0 it never reports the real port.

Each is fixable in the fork, but a small transport of our own is less work
than carrying those patches and has a better security story.

## Wanted

- `--automation-socket <path>` (runtime switch, off by default — **not** a
  Cargo feature): a Unix domain socket, created mode `0600`, default under
  `$XDG_RUNTIME_DIR`; an existing socket file is an error unless stale.
- Line-delimited JSON-RPC carrying `Request` / `Response`; subscriptions
  for the event logs stream notifications on the same connection.
- The listener runs on the shared async runtime and feeds the executor's
  queue over a channel; responses are written as the executor answers, so
  several requests may be in flight.
- A `hello` returning the protocol version and viewer identity (agent,
  grid, pid).
- Bind or accept failures are logged as errors and fail start-up when the
  switch was given — never silently.

Acceptance: a launched viewer answers `hello` on its socket; a request opens
a floater and the response arrives when done; two viewer processes run side
by side with their own sockets; no socket exists without the switch.
